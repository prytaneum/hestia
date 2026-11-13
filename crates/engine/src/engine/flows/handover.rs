//! Staging the official plugins an app update that changes the plugin API
//! needs, so the daemon that exposes the new API installs them before it loads
//! any plugin ([0112](../../../../../docs/decisions/0112-the-app-and-official-plugins-update-together.md)).

use std::path::Path;

use anyhow::{Context, Result};
use proto::error::ErrorInfo;
use proto::net::NetworkState;
use sha2::{Digest, Sha256};

use super::repository::{file_name, SIGNATURE_CAP};
use super::Engine;
use crate::checksum::hex;
use crate::minecraft::materialize;
use crate::plugins::{
    build_url, plan_handover, verify_build, Carry, Handover, Installed, Source, PACKAGE_EXTENSION,
    SIGNATURE_SUFFIX,
};

impl Engine {
    /// Downloads and verifies, into the staging directory for `api`, a build
    /// for that plugin API of every installed official plugin that would not
    /// run on it. Returns how many were staged; none when they all still run.
    /// Refuses with [`ErrorInfo::UpdatePluginsIncompatible`] when any has no
    /// build for `api`, leaving nothing staged: the app update must not be
    /// applied over a launcher that could then create nothing.
    pub async fn stage_official_plugins(&self, api: &str) -> Result<usize> {
        self.update.discard_staged_plugins();
        let keys = common::app::official_plugin_pubkeys();
        let broken = self.plugins().official_broken_by(api, &keys);
        if broken.is_empty() {
            tracing::debug!(%api, "every official plugin runs on the new plugin API");
            return Ok(0);
        }
        let synced = if self.network().status().state == NetworkState::Offline {
            self.local_repositories()
        } else {
            self.sync_repositories(None).await?
        };
        let sources = synced
            .into_iter()
            .filter_map(|s| s.source)
            .collect::<Vec<_>>();
        self.stage_given(api, &keys, &broken, &sources).await
    }

    async fn stage_given(
        &self,
        api: &str,
        keys: &[&str],
        broken: &[Installed],
        sources: &[Source],
    ) -> Result<usize> {
        let Handover { carry, stranded } = plan_handover(sources, broken, api);
        if !stranded.is_empty() {
            tracing::warn!(%api, plugins = ?stranded, "official plugins have no build for the new plugin API");
            return Err(ErrorInfo::UpdatePluginsIncompatible {
                api: api.to_string(),
                plugins: stranded,
            }
            .into());
        }

        let dir = self.update.staged_plugins(api);
        tokio::fs::create_dir_all(&dir)
            .await
            .context("cannot create the plugin staging directory")?;
        for carry in &carry {
            if let Err(error) = self.stage_carry(&dir, keys, carry).await {
                self.update.discard_staged_plugins();
                return Err(error);
            }
        }
        tracing::info!(%api, staged = carry.len(), "official plugins staged for the update");
        Ok(carry.len())
    }

    async fn stage_carry(&self, dir: &Path, keys: &[&str], carry: &Carry) -> Result<()> {
        let refused = |detail: String| ErrorInfo::RepositoryRefused { detail };
        let url = build_url(&carry.repository, &carry.build.file).map_err(ErrorInfo::from)?;
        let file = file_name(&url)?;
        materialize::validate_filename(&file)?;
        if !file.ends_with(&format!(".{PACKAGE_EXTENSION}")) {
            return Err(refused(format!("`{file}` is not a plugin package")).into());
        }

        let downloads = self.plugin_downloads();
        let token = downloads.fetch(&url).await?;
        let bytes = tokio::fs::read(downloads.staged(&token)?.path).await;
        downloads.discard(&token);
        let bytes = bytes?;
        if !hex(&Sha256::digest(&bytes)).eq_ignore_ascii_case(&carry.build.sha256) {
            return Err(refused(
                "the download does not match the checksum the repository lists".into(),
            )
            .into());
        }
        let signature = downloads
            .fetch_capped(&format!("{url}{SIGNATURE_SUFFIX}"), SIGNATURE_CAP)
            .await
            .map_err(|error| refused(format!("the build has no signature: {error:#}")))?;
        let keys: Vec<String> = keys.iter().map(|key| key.to_string()).collect();
        let text = std::str::from_utf8(&signature)
            .map_err(|_| refused("the signature is not text".into()))?;
        verify_build(&bytes, text, &keys, &carry.id, &carry.build.version, &file)
            .map_err(ErrorInfo::from)?;

        tokio::fs::write(dir.join(format!("{file}{SIGNATURE_SUFFIX}")), &signature).await?;
        tokio::fs::write(dir.join(&file), &bytes).await?;
        tracing::debug!(plugin = %carry.id, version = %carry.build.version, "official plugin build staged");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::testkit::{sample_index, Signer};
    use crate::plugins::{key_id_of, PluginRecord};

    const NEXT_API: &str = "2.0.0";

    fn install(engine: &Engine, id: &str, version: &str, api: &str, signer: &Signer) {
        let record = PluginRecord {
            id: id.to_string(),
            signer: Some(key_id_of(&signer.key()).unwrap()),
            ..PluginRecord::default()
        };
        engine.plugins().save(&record).unwrap();
        let package = engine.plugins().package_dir(id).unwrap();
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("plugin.toml"),
            format!("id = \"{id}\"\nversion = \"{version}\"\napi = \"^{api}\"\n"),
        )
        .unwrap();
    }

    fn listed_by(signer: &Signer) -> Vec<Source> {
        let url = "https://handover.example/plugins.json";
        let index = serde_json::from_slice(&sample_index(&signer.key(), |_| {})).unwrap();
        vec![Source::new(url.to_string(), &signer.key(), index)]
    }

    #[tokio::test]
    async fn a_plugin_api_every_plugin_runs_on_stages_nothing() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));
        let stale = engine.update.staged_plugins("9.9.9");
        std::fs::create_dir_all(&stale).unwrap();

        let staged = engine
            .stage_official_plugins(proto::plugin::API_VERSION)
            .await
            .unwrap();

        assert_eq!(staged, 0);
        assert!(!stale.exists());
    }

    #[tokio::test]
    async fn an_official_plugin_without_a_build_for_the_new_api_refuses_the_update() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));
        let official = Signer::new();
        install(
            &engine,
            "aa",
            "0.1.0",
            proto::plugin::API_VERSION,
            &official,
        );
        let key = official.key();
        let keys = [key.as_str()];
        let broken = engine.plugins().official_broken_by(NEXT_API, &keys);
        assert_eq!(broken.len(), 1);

        let refusal = engine
            .stage_given(NEXT_API, &keys, &broken, &listed_by(&official))
            .await
            .unwrap_err();

        let info = crate::error_info(refusal);
        assert!(
            matches!(
                &info,
                ErrorInfo::UpdatePluginsIncompatible { api, plugins }
                    if api == NEXT_API && plugins == &["aa".to_string()]
            ),
            "{info:?}"
        );
        assert!(!engine.update.staged_plugins(NEXT_API).exists());
    }

    #[test]
    fn only_official_plugins_the_new_api_cannot_run_are_carried() {
        let home = tempfile::tempdir().unwrap();
        let engine = Engine::new(Some(home.path()));
        let (official, third_party) = (Signer::new(), Signer::new());
        install(&engine, "aa", "0.1.0", "1.0.0", &official);
        install(&engine, "bb", "0.1.0", NEXT_API, &official);
        install(&engine, "cc", "0.1.0", "1.0.0", &third_party);
        let key = official.key();

        let broken = engine
            .plugins()
            .official_broken_by(NEXT_API, &[key.as_str()]);

        assert_eq!(
            broken.iter().map(|b| b.id.as_str()).collect::<Vec<_>>(),
            ["aa"]
        );
    }
}
