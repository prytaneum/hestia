use std::path::Path;

use engine::Engine;
use proto::content::{ContentAddSpec, ContentDone, ContentFailure, ContentKind, InstalledContent};
use proto::error::EntryKind;
use proto::job::Job;
use proto::minecraft::ProvisionProgress;

use super::job::{Jobs, Spec};
use crate::runtime::Claim;

/// One content install or update for one entry — what `Jobs::start_content`
/// runs off-thread. The side is a field, not a second copy of every verb.
pub enum ContentJob {
    Add {
        entry: Entry,
        spec: ContentAddSpec,
    },
    /// `items` empty updates every platform-sourced item of the kind; a `path`
    /// updates the one named local item from that file.
    Update {
        entry: Entry,
        kind: ContentKind,
        items: Vec<String>,
        path: String,
    },
    /// Re-pin one item to a specific published version.
    SetVersion {
        entry: Entry,
        kind: ContentKind,
        item: String,
        version: String,
        source: String,
    },
}

/// Which entry a job is for, owned — the job outlives the request that asked
/// for it, so it cannot borrow the reference.
pub struct Entry {
    pub kind: EntryKind,
    pub id: String,
    pub name: String,
}

impl From<engine::ServerRecord> for Entry {
    fn from(record: engine::ServerRecord) -> Self {
        Entry {
            kind: EntryKind::Server,
            id: record.id,
            name: record.name,
        }
    }
}

impl From<engine::InstanceRecord> for Entry {
    fn from(record: engine::InstanceRecord) -> Self {
        Entry {
            kind: EntryKind::Instance,
            id: record.id,
            name: record.name,
        }
    }
}

impl Entry {
    pub fn entry_ref(&self) -> engine::EntryRef<'_> {
        match self.kind {
            EntryKind::Server => engine::EntryRef::Server(&self.id),
            EntryKind::Instance => engine::EntryRef::Instance(&self.id),
        }
    }
}

type Installed = (Vec<InstalledContent>, Vec<ContentFailure>);

impl ContentJob {
    fn id_prefix(&self) -> &'static str {
        match self {
            ContentJob::Add { entry, .. } => match entry.kind {
                EntryKind::Server => "server-content-add",
                EntryKind::Instance => "instance-content-add",
            },
            ContentJob::Update { entry, .. } => match entry.kind {
                EntryKind::Server => "server-content-update",
                EntryKind::Instance => "instance-content-update",
            },
            ContentJob::SetVersion { entry, .. } => match entry.kind {
                EntryKind::Server => "server-content-set-version",
                EntryKind::Instance => "instance-content-set-version",
            },
        }
    }

    async fn run(
        self,
        engine: &Engine,
        on_progress: &engine::Job<'_>,
    ) -> anyhow::Result<Installed> {
        match self {
            ContentJob::Add { entry, spec } => {
                engine
                    .add_entry_content(entry.entry_ref(), &spec, on_progress)
                    .await
            }
            ContentJob::Update {
                entry,
                kind,
                items,
                path,
            } => match items.first() {
                Some(item) if !path.is_empty() => engine
                    .update_entry_content_from_file(entry.entry_ref(), kind, item, Path::new(&path))
                    .map(|item| (vec![item], Vec::new())),
                _ => {
                    engine
                        .update_entry_content(entry.entry_ref(), kind, &items, on_progress)
                        .await
                }
            },
            ContentJob::SetVersion {
                entry,
                kind,
                item,
                version,
                source,
            } => engine
                .set_entry_content_version(
                    entry.entry_ref(),
                    kind,
                    &item,
                    &version,
                    &source,
                    on_progress,
                )
                .await
                .map(|items| (items, Vec::new())),
        }
    }
}

impl Jobs {
    /// Start an install/update job off-thread, holding the entry's claim until
    /// it settles.
    pub fn start_content<C>(&self, job: ContentJob, id: String, claim: Claim) -> String
    where
        C: Job<Progress = ProvisionProgress, Done = ContentDone>,
    {
        let spec = Spec {
            id,
            prefix: job.id_prefix(),
            claim: Some(claim),
        };

        self.run::<C, _>(spec, move |engine, reporter| {
            Box::pin(async move {
                let (items, failures) = job.run(engine, &reporter.job()).await?;
                Ok(ContentDone { items, failures })
            })
        })
    }
}
