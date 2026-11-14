//! A provider has one id, `<plugin>.<id>`, and a bare name is a shorthand the
//! roster resolves: core's own first, then an official plugin's, then the only
//! one there is. Uses the `static-index` fixture, whose provider id is `index`,
//! installed as several plugins.

use async_trait::async_trait;
use engine::{ContentProvider, Standing};
use proto::content::{ContentProject, ContentVersion, SearchQuery, SearchResult, VersionQuery};

use super::content_source::load_as;
use super::*;

const BARE: &str = "index";

struct Core;

#[async_trait]
impl ContentProvider for Core {
    fn id(&self) -> &str {
        BARE
    }
    fn name(&self) -> &str {
        "Core index"
    }
    fn kinds(&self) -> Vec<ContentKind> {
        vec![ContentKind::DataPack]
    }
    async fn parse_url(&self, _url: &str) -> Option<engine::UrlRef> {
        None
    }
    async fn search(&self, _query: &SearchQuery) -> anyhow::Result<SearchResult> {
        Ok(SearchResult::default())
    }
    async fn project(
        &self,
        _project: &str,
        _kind: Option<ContentKind>,
    ) -> anyhow::Result<ContentProject> {
        anyhow::bail!("the core index has no such project")
    }
    async fn versions(&self, _query: &VersionQuery) -> anyhow::Result<Vec<ContentVersion>> {
        Ok(Vec::new())
    }
}

struct Rig {
    engine: engine::Engine,
    instance: String,
    home: tempfile::TempDir,
    files: fixture::Files,
}

impl Rig {
    async fn new(core: bool) -> Rig {
        let home = tempfile::tempdir().expect("temp home");
        let files = fixture::Files::serve().await;
        let core: Vec<Box<dyn ContentProvider>> = if core { vec![Box::new(Core)] } else { vec![] };
        let engine = engine::Engine::over(
            Some(home.path()),
            engine::Minecraft::with_providers(vec![Box::new(fixture::Flavor::resolving("fabric"))]),
            engine::Content::with_providers(core),
        );
        let instance = engine
            .create_instance("modded", "fabric", "1.21", None, &[])
            .await
            .expect("create the instance")
            .id;
        Rig {
            engine,
            instance,
            home,
            files,
        }
    }

    async fn install(&self, plugin: &str, standing: Standing) {
        let (loaded, manifest) = load_as(plugin, self.home.path(), &self.files, &["127.0.0.1"]);
        engine::Contributions::connect(&loaded, &manifest, standing)
            .await
            .register(&self.engine, plugin);
    }

    fn resolved(&self, name: &str) -> anyhow::Result<String> {
        self.engine.content().canonical(name)
    }

    async fn install_from(&self, source: &str) -> proto::content::ContentDone {
        let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
        let mut spec = add(&["cobble-tweaks"]);
        spec.source = source.into();
        self.engine
            .add_entry_content(
                EntryRef::Instance(&self.instance),
                &spec,
                &NO_CHAIN,
                &job(&cancel, &report),
            )
            .await
            .expect("the batch runs")
    }
}

#[tokio::test]
async fn a_bare_name_reaches_core_then_the_official_plugin_and_never_a_third_party() {
    let rig = Rig::new(true).await;
    rig.install("other-index", Standing::Community).await;
    rig.install("hestia-index", Standing::Official).await;

    assert_eq!(rig.resolved(BARE).unwrap(), BARE, "core answers first");
    assert_eq!(
        rig.resolved("hestia-index.index").unwrap(),
        "hestia-index.index"
    );
    assert_eq!(
        rig.resolved("other-index.index").unwrap(),
        "other-index.index"
    );

    let core_less = Rig::new(false).await;
    core_less.install("other-index", Standing::Community).await;
    assert_eq!(
        core_less.resolved(BARE).unwrap(),
        "other-index.index",
        "the only provider of a name is reached by it"
    );
    core_less.install("hestia-index", Standing::Official).await;
    assert_eq!(core_less.resolved(BARE).unwrap(), "hestia-index.index");

    let done = core_less.install_from(BARE).await;
    assert!(done.failures.is_empty(), "{:?}", done.failures);
    let [item] = done.items.as_slice() else {
        panic!("one item expected: {:?}", done.items);
    };
    assert_eq!(item.source, "hestia-index.index", "the full id is stored");
    assert_eq!(
        core_less.resolved("other-index.index").unwrap(),
        "other-index.index"
    );

    assert_eq!(core_less.engine.content().unregister("hestia-index"), 1);
    assert_eq!(core_less.resolved(BARE).unwrap(), "other-index.index");
}

#[tokio::test]
async fn a_bare_name_two_unofficial_plugins_share_is_ambiguous_and_names_both() {
    let rig = Rig::new(false).await;
    rig.install("acme-index", Standing::Community).await;
    rig.install("beta-index", Standing::Community).await;

    let error = rig
        .resolved(BARE)
        .expect_err("which one is not for the launcher to guess");
    let ErrorInfo::ProviderAmbiguous { name, candidates } = engine::error_info(error) else {
        panic!("expected an ambiguity");
    };
    assert_eq!(name, BARE);
    assert_eq!(candidates, ["acme-index.index", "beta-index.index"]);

    assert_eq!(
        rig.resolved("beta-index.index").unwrap(),
        "beta-index.index"
    );
    assert!(matches!(
        engine::error_info(rig.resolved("nope").unwrap_err()),
        ErrorInfo::ContentSourceUnknown { .. }
    ));
}
