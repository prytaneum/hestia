//! Materialises resolved profile pieces onto disk: single artifacts (jars),
//! Maven-layout libraries, and the shared asset store. A launch pays only for
//! what is missing: an artifact is skipped when present at the expected size,
//! and libraries and assets are fetched from the lists `plan` found missing.

use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
use proto::download::{Checksum, HashAlgorithm};
use proto::minecraft::{Artifact, ProvisionPhase, ProvisionProgress};
use serde_json::Value;

use super::plan::{AssetObject, Meter, Tally};
use super::profile::{AssetIndex, Library, Native};

use crate::cache::Cache;
use crate::download::Downloader;

const ASSET_HOST: &str = "https://resources.download.minecraft.net";
const CONCURRENT_FETCHES: usize = 16;

/// What a long-running step reports through — and is cancelled by. The two
/// travel together deliberately: a step that reports progress is exactly one
/// that can be stopped between reports (`crate::cancel`).
pub type OnProgress<'a> = &'a crate::cancel::Job<'a>;

pub fn validate_filename(name: &str) -> Result<()> {
    if name.is_empty() || name.starts_with('.') || name.contains(['/', '\\', '"']) {
        bail!("artifact has an unsafe filename: '{name}'");
    }
    Ok(())
}

/// Join an upstream-supplied relative path under `root`, rejecting anything
/// absolute or traversing.
pub fn safe_join(root: &Path, relative: &str) -> Result<PathBuf> {
    let rel = Path::new(relative);
    if relative.is_empty()
        || rel.is_absolute()
        || rel.components().any(|c| !matches!(c, Component::Normal(_)))
    {
        bail!("unsafe artifact path: '{relative}'");
    }
    Ok(root.join(rel))
}

pub(crate) fn present(path: &Path, size: u64) -> bool {
    match std::fs::metadata(path) {
        Ok(m) => m.is_file() && (size == 0 || m.len() == size),
        Err(_) => false,
    }
}

/// Ensure one artifact at `destination`, reporting byte progress under `phase`.
pub async fn ensure_artifact(
    cache: Option<&Cache>,
    artifact: &Artifact,
    destination: &Path,
    phase: ProvisionPhase,
    tally: Option<&Tally>,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    if present(destination, artifact.size) {
        return Ok(());
    }
    on_progress.check()?;
    let meter = Meter::new(tally);
    Downloader::new(cache)
        .fetch(
            &artifact.url,
            destination,
            artifact.checksum.as_ref(),
            &|dp| {
                // Per chunk: a large artifact stops promptly, and its `.part`
                // is discarded by the failure path rather than promoted.
                on_progress.check()?;
                meter.reach(dp.downloaded);
                on_progress.report(&ProvisionProgress {
                    phase,
                    current: dp.downloaded,
                    total: dp.total,
                    detail: artifact.filename.clone(),
                    ..ProvisionProgress::default()
                });
                Ok(())
            },
        )
        .await?;
    meter.reach(artifact.size);
    Ok(())
}

/// A batch of files fetched side by side, reported as one phase. With a
/// `tally` the phase carries bytes (`current`/`total`) and the file count
/// (`item`/`items`); without one it carries only completed/total files.
struct Fetched<'a> {
    job: OnProgress<'a>,
    phase: ProvisionPhase,
    tally: Option<&'a Tally>,
    files_total: u64,
    bytes_total: u64,
    files: AtomicU64,
    bytes: AtomicU64,
}

impl<'a> Fetched<'a> {
    fn new(
        job: OnProgress<'a>,
        phase: ProvisionPhase,
        tally: Option<&'a Tally>,
        files_total: u64,
        bytes_total: u64,
    ) -> Self {
        Fetched {
            job,
            phase,
            tally,
            files_total,
            bytes_total,
            files: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
        }
    }

    fn meter(&self) -> Meter<'a> {
        Meter::new(self.tally)
    }

    fn written(&self, meter: &Meter<'_>, bytes: u64, detail: &str) {
        self.bytes.fetch_add(meter.reach(bytes), Ordering::Relaxed);
        self.report(detail);
    }

    fn completed(&self, meter: &Meter<'_>, size: u64, detail: &str) {
        self.bytes.fetch_add(meter.reach(size), Ordering::Relaxed);
        self.files.fetch_add(1, Ordering::Relaxed);
        self.report(detail);
    }

    fn report(&self, detail: &str) {
        let files = self.files.load(Ordering::Relaxed);
        let progress = match self.tally {
            Some(_) => ProvisionProgress {
                phase: self.phase,
                current: self.bytes.load(Ordering::Relaxed),
                total: self.bytes_total,
                detail: detail.to_string(),
                item: files,
                items: self.files_total,
                ..ProvisionProgress::default()
            },
            None => ProvisionProgress {
                phase: self.phase,
                current: files,
                total: self.files_total,
                detail: detail.to_string(),
                ..ProvisionProgress::default()
            },
        };
        self.job.report(&progress);
    }
}

/// Fetch the libraries a plan found missing under `root` (Maven layout).
pub async fn ensure_libraries(
    cache: Option<&Cache>,
    missing: &[Library],
    root: &Path,
    tally: Option<&Tally>,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    if missing.is_empty() {
        return Ok(());
    }
    let fetched = Fetched::new(
        on_progress,
        ProvisionPhase::Libraries,
        tally,
        missing.len() as u64,
        missing.iter().map(|l| l.artifact.size).sum(),
    );
    fetched.report("");

    // Owned items: a closure taking a reference and returning an async block
    // trips rustc's higher-ranked lifetime inference (rust-lang/rust#89976).
    let fetches = missing.iter().cloned().map(|library| {
        let fetched = &fetched;
        async move {
            on_progress.check()?;
            let destination = safe_join(root, &library.path)?;
            let meter = fetched.meter();
            Downloader::new(cache)
                .fetch(
                    &library.artifact.url,
                    &destination,
                    library.artifact.checksum.as_ref(),
                    &|dp| {
                        on_progress.check()?;
                        fetched.written(&meter, dp.downloaded, &library.name);
                        Ok(())
                    },
                )
                .await
                .with_context(|| format!("library {}", library.name))?;
            fetched.completed(&meter, library.artifact.size, &library.name);
            Ok::<(), anyhow::Error>(())
        }
    });
    drain(fetches).await
}

/// Unpack every native library, already downloaded under `libraries_root`, into
/// `natives_dir`, which is what `-Djava.library.path` names.
pub fn ensure_natives(
    natives: &[Native],
    libraries_root: &Path,
    natives_dir: &Path,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    if natives.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(natives_dir)
        .with_context(|| format!("cannot create {}", natives_dir.display()))?;
    for native in natives {
        on_progress.check()?;
        let jar = safe_join(libraries_root, &native.library.path)?;
        unpack_native(&jar, natives_dir, &native.exclude)
            .with_context(|| format!("native library {}", native.library.name))?;
    }
    Ok(())
}

/// Unpack one native jar. An entry whose path escapes `dest` is refused.
fn unpack_native(jar: &Path, dest: &Path, exclude: &[String]) -> Result<()> {
    let file =
        std::fs::File::open(jar).with_context(|| format!("cannot open {}", jar.display()))?;
    let mut archive = zip::ZipArchive::new(file).context("opening the native jar")?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index)?;
        if entry.is_dir() {
            continue;
        }
        let name = entry.name()?.into_owned();
        if exclude
            .iter()
            .any(|prefix| name.starts_with(prefix.as_str()))
        {
            continue;
        }
        let Some(relative) = entry.enclosed_name() else {
            bail!("native jar contains an unsafe path: '{name}'");
        };
        let out = dest.join(relative);
        if present(&out, entry.size()) {
            continue;
        }
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent)?;
        }
        // The natives root is shared by every session on this version, so a
        // concurrent launch must never open one of these mid-write.
        let staging = common::atomic::staged(&out);
        let mut writer = std::fs::File::create(&staging)
            .with_context(|| format!("cannot write {}", staging.display()))?;
        std::io::copy(&mut entry, &mut writer)?;
        drop(writer);
        std::fs::rename(&staging, &out)
            .with_context(|| format!("cannot commit {}", out.display()))?;
    }
    Ok(())
}

/// An asset index read from disk: every object it names, and the directory the
/// game must be pointed at (`${game_assets}`) when the index declares a legacy
/// layout — a client too old to read the hashed store wants a tree named the
/// way the index names it. `None` when the modern store is what it reads.
pub struct AssetStore {
    pub mapped: Option<PathBuf>,
    pub objects: Vec<AssetObject>,
}

/// Fetch the asset index (`indexes/<id>.json`) when missing and read it. It is
/// small, and a launch needs its objects to know what it has to download.
pub async fn read_assets(
    cache: Option<&Cache>,
    index: &AssetIndex,
    root: &Path,
    game_dir: &Path,
    on_progress: OnProgress<'_>,
) -> Result<AssetStore> {
    validate_filename(&index.id)?;
    let index_path = root.join("indexes").join(format!("{}.json", index.id));
    if !present(&index_path, index.artifact.size) {
        on_progress.report(&ProvisionProgress {
            phase: ProvisionPhase::Resolving,
            detail: "asset index".to_string(),
            ..ProvisionProgress::default()
        });
        Downloader::new(cache)
            .fetch(
                &index.artifact.url,
                &index_path,
                index.artifact.checksum.as_ref(),
                &|_| Ok(()),
            )
            .await
            .context("asset index")?;
    }

    let text = std::fs::read_to_string(&index_path)
        .with_context(|| format!("cannot read {}", index_path.display()))?;
    let parsed: Value = serde_json::from_str(&text).context("asset index is malformed JSON")?;
    // Two upstream spellings of one requirement: an index flagged either way
    // names assets its client reads by path, not by hash.
    let mapped = if parsed.get("map_to_resources").and_then(Value::as_bool) == Some(true) {
        Some(game_dir.join("resources"))
    } else if parsed.get("virtual").and_then(Value::as_bool) == Some(true) {
        Some(root.join("virtual").join(&index.id))
    } else {
        None
    };
    let objects = parsed
        .get("objects")
        .and_then(Value::as_object)
        .context("asset index has no objects map")?;

    let mut entries = Vec::with_capacity(objects.len());
    for (name, object) in objects {
        let Some(hash) = object.get("hash").and_then(Value::as_str) else {
            continue;
        };
        if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("asset index names an invalid object hash: '{hash}'");
        }
        entries.push(AssetObject {
            name: name.clone(),
            hash: hash.to_string(),
            size: object.get("size").and_then(Value::as_u64).unwrap_or(0),
        });
    }
    Ok(AssetStore {
        mapped,
        objects: entries,
    })
}

/// Fetch the objects a plan found missing into the content-addressed store under
/// `root` (`objects/<hh>/<hash>`), shared by every version and never fetched
/// twice, then mirror the whole index into the legacy tree when it declares one.
pub async fn ensure_assets(
    store: &AssetStore,
    missing: &[AssetObject],
    root: &Path,
    index_id: &str,
    tally: Option<&Tally>,
    on_progress: OnProgress<'_>,
) -> Result<()> {
    let objects_root = root.join("objects");
    let fetched = Fetched::new(
        on_progress,
        ProvisionPhase::Assets,
        tally,
        missing.len() as u64,
        missing.iter().map(|o| o.size).sum(),
    );
    if !missing.is_empty() {
        fetched.report(index_id);
    }

    let fetches = missing.iter().cloned().map(|object| {
        let fetched = &fetched;
        let objects_root = &objects_root;
        async move {
            on_progress.check()?;
            let checksum = Checksum {
                algorithm: HashAlgorithm::Sha1,
                hex: object.hash.clone(),
            };
            let meter = fetched.meter();
            // The objects tree is itself content-addressed; going through
            // the download cache would only store every asset twice.
            Downloader::new(None)
                .fetch(
                    &format!("{ASSET_HOST}/{}/{}", &object.hash[..2], object.hash),
                    &object.path(objects_root),
                    Some(&checksum),
                    &|dp| {
                        on_progress.check()?;
                        fetched.written(&meter, dp.downloaded, index_id);
                        Ok(())
                    },
                )
                .await
                .with_context(|| format!("asset object {}", object.hash))?;
            fetched.completed(&meter, object.size, index_id);
            Ok::<(), anyhow::Error>(())
        }
    });
    drain(fetches).await?;

    if let Some(dir) = &store.mapped {
        for object in &store.objects {
            let destination = safe_join(dir, &object.name)?;
            if present(&destination, object.size) {
                continue;
            }
            if let Some(parent) = destination.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::copy(object.path(&objects_root), &destination)
                .with_context(|| format!("cannot write {}", destination.display()))?;
        }
    }
    Ok(())
}

/// Run the fetch futures a few at a time, failing fast on the first error.
async fn drain<I, F>(fetches: I) -> Result<()>
where
    I: Iterator<Item = F>,
    F: std::future::Future<Output = Result<()>>,
{
    let mut stream = futures_util::stream::iter(fetches).buffer_unordered(CONCURRENT_FETCHES);
    while let Some(result) = stream.next().await {
        result?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use super::*;

    fn native_jar(dir: &Path, entries: &[(&str, &[u8])]) -> PathBuf {
        let path = dir.join("native.jar");
        let file = std::fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, body) in entries {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(body).unwrap();
        }
        zip.finish().unwrap();
        path
    }

    #[test]
    fn unpacking_skips_the_excluded_prefixes() {
        let temp = tempfile::tempdir().unwrap();
        let jar = native_jar(
            temp.path(),
            &[
                ("liblwjgl.so", b"binary"),
                ("META-INF/MANIFEST.MF", b"manifest"),
                ("META-INF/SIG.RSA", b"signature"),
            ],
        );
        let dest = temp.path().join("natives");
        unpack_native(&jar, &dest, &["META-INF/".to_string()]).unwrap();

        assert!(dest.join("liblwjgl.so").is_file());
        assert!(!dest.join("META-INF").exists());
    }

    #[test]
    fn unpacking_replaces_a_truncated_file_and_leaves_a_good_one() {
        let temp = tempfile::tempdir().unwrap();
        let jar = native_jar(temp.path(), &[("liblwjgl.so", b"binary")]);
        let dest = temp.path().join("natives");
        std::fs::create_dir_all(&dest).unwrap();
        let target = dest.join("liblwjgl.so");
        std::fs::write(&target, b"tru").unwrap();

        unpack_native(&jar, &dest, &[]).unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"binary");

        let before = std::fs::metadata(&target).unwrap().modified().unwrap();
        unpack_native(&jar, &dest, &[]).unwrap();
        assert_eq!(
            std::fs::metadata(&target).unwrap().modified().unwrap(),
            before,
            "a file already the right size is not rewritten"
        );
    }
}
