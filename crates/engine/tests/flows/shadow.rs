//! An official plugin's provider takes the id of a built-in and the built-in
//! answers again once the plugin is gone, using the `static-index` fixture
//! whose provider id is `index`.

use async_trait::async_trait;
use engine::ContentProvider;
use proto::content::{ContentProject, ContentVersion, SearchQuery, SearchResult, VersionQuery};

use super::content_source::{load, manifest};
use super::*;

const SHADOWED: &str = "index";
const NAMESPACED: &str = "static-index.index";

struct Builtin;

#[async_trait]
impl ContentProvider for Builtin {
    fn id(&self) -> &str {
        SHADOWED
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

async fn rig(official: bool, hosts: &[&str]) -> (engine::Engine, String, tempfile::TempDir) {
    let home = tempfile::tempdir().expect("temp home");
    let files = fixture::Files::serve().await;
    let engine = engine::Engine::over(
        Some(home.path()),
        engine::Minecraft::with_providers(vec![Box::new(fixture::Flavor::resolving("fabric"))]),
        engine::Content::with_providers(vec![Box::new(Builtin)]),
    );
    let id = engine
        .create_instance("modded", "fabric", "1.21", None, &[])
        .await
        .expect("create the instance")
        .id;
    let (plugin, manifest) = load(home.path(), &files, hosts);
    engine::Contributions::connect(&plugin, &manifest, official)
        .await
        .register(&engine, &manifest.id);
    (engine, id, home)
}

fn offered(engine: &engine::Engine, id: &str) -> Vec<String> {
    engine
        .content()
        .sources()
        .into_iter()
        .filter(|source| source.id == id)
        .map(|source| source.name)
        .collect()
}

async fn installs_from_index(engine: &engine::Engine, id: &str) -> bool {
    let (cancel, report) = (Cancel::new(), |_: &ProvisionProgress| {});
    let mut spec = add(&["cobble-tweaks"]);
    spec.source = SHADOWED.into();
    let done = engine
        .add_entry_content(
            EntryRef::Instance(id),
            &spec,
            &NO_CHAIN,
            &job(&cancel, &report),
        )
        .await
        .expect("the batch runs");
    done.failures.is_empty() && done.items.len() == 1
}

#[tokio::test]
async fn an_official_source_answers_for_the_builtin_until_it_is_unregistered() {
    let (engine, id, _home) = rig(true, &["127.0.0.1"]).await;

    assert_eq!(offered(&engine, SHADOWED), ["Static index"]);
    assert!(offered(&engine, NAMESPACED).is_empty());
    assert!(installs_from_index(&engine, &id).await);

    assert_eq!(engine.content().unregister(&manifest().id), 1);

    assert_eq!(offered(&engine, SHADOWED), ["Core index"]);
    assert!(!installs_from_index(&engine, &id).await);
}

#[tokio::test]
async fn a_source_that_is_not_official_keeps_its_namespace_and_the_builtin() {
    let (engine, _id, _home) = rig(false, &["127.0.0.1"]).await;

    assert_eq!(offered(&engine, SHADOWED), ["Core index"]);
    assert_eq!(offered(&engine, NAMESPACED), ["Static index"]);
}

#[tokio::test]
async fn a_shadowing_source_stays_inside_the_hosts_it_was_granted() {
    let (engine, _id, _home) = rig(true, &["index.example.com"]).await;

    let versions = engine
        .content()
        .versions(&VersionQuery {
            source: SHADOWED.into(),
            project: "cobble-tweaks".into(),
            ..VersionQuery::default()
        })
        .await
        .expect("the lookup itself works");

    assert!(versions.is_empty(), "{versions:?}");
}
