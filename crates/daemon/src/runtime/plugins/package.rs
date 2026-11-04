//! The package a plugin inspect or install names: a local path, or a file the
//! daemon downloaded from a link and holds under a token.

use std::path::{Path, PathBuf};

use engine::{Engine, Staged};
use proto::error::ErrorInfo;
use proto::plugin::{PluginInstallParams, PluginPackageRef};

use crate::runtime::router::ServiceResult;

pub enum Package {
    Local(PathBuf),
    Downloaded { token: String, staged: Staged },
}

impl Package {
    /// Downloads the link of an inspect request; nothing else touches the network.
    pub async fn fetch(engine: &Engine, request: &PluginPackageRef) -> ServiceResult<Package> {
        if request.url.is_empty() {
            return Ok(Package::Local(PathBuf::from(&request.path)));
        }
        if !request.path.is_empty() {
            return Err(both());
        }
        let downloads = engine.plugin_downloads();
        let token = downloads
            .fetch(&request.url)
            .await
            .map_err(crate::runtime::engine_error)?;
        let staged = downloads
            .staged(&token)
            .map_err(crate::runtime::engine_error)?;
        Ok(Package::Downloaded { token, staged })
    }

    pub fn held(engine: &Engine, request: &PluginInstallParams) -> ServiceResult<Package> {
        if request.staged.is_empty() {
            return Ok(Package::Local(PathBuf::from(&request.path)));
        }
        if !request.path.is_empty() {
            return Err(both());
        }
        let staged = engine
            .plugin_downloads()
            .staged(&request.staged)
            .map_err(crate::runtime::engine_error)?;
        Ok(Package::Downloaded {
            token: request.staged.clone(),
            staged,
        })
    }

    pub fn path(&self) -> &Path {
        match self {
            Package::Local(path) => path,
            Package::Downloaded { staged, .. } => &staged.path,
        }
    }

    pub fn token(&self) -> Option<&str> {
        match self {
            Package::Local(_) => None,
            Package::Downloaded { token, .. } => Some(token),
        }
    }
}

fn both() -> ErrorInfo {
    ErrorInfo::PluginInvalid {
        detail: "give a path or a link, not both".to_string(),
    }
}
