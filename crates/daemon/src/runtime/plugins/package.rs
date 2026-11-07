//! The package a plugin inspect or install names: a local path, or a file the
//! daemon downloaded from a link and holds under a token.

use std::path::{Path, PathBuf};

use engine::{Engine, Staged};
use proto::error::ErrorInfo;
use proto::plugin::{PluginInstallParams, PluginPackageRef};
use proto::warning::WarningInfo;

use crate::runtime::router::ServiceResult;

pub enum Package {
    Local(PathBuf),
    /// A folder run in place, never copied.
    Dev(PathBuf),
    Downloaded {
        token: String,
        staged: Staged,
    },
}

impl Package {
    /// Downloads the link of an inspect request; nothing else touches the network.
    pub async fn fetch(engine: &Engine, request: &PluginPackageRef) -> ServiceResult<Package> {
        let from_repo = !request.repo.is_empty();
        if request.url.is_empty() && !from_repo {
            return Ok(Package::Local(PathBuf::from(&request.path)));
        }
        if !request.path.is_empty() || (from_repo && !request.url.is_empty()) {
            return Err(both());
        }
        let downloads = engine.plugin_downloads();
        let token = if from_repo {
            engine
                .stage_repository_build(&request.repo, &request.plugin)
                .await
        } else {
            downloads.fetch(&request.url).await
        }
        .map_err(crate::runtime::engine_error)?;
        let staged = downloads
            .staged(&token)
            .map_err(crate::runtime::engine_error)?;
        Ok(Package::Downloaded { token, staged })
    }

    pub fn held(engine: &Engine, request: &PluginInstallParams) -> ServiceResult<Package> {
        if request.dev {
            if !request.staged.is_empty() {
                return Err(ErrorInfo::PluginInvalid {
                    detail: "a dev install runs a folder, not a download".to_string(),
                });
            }
            return Ok(Package::Dev(PathBuf::from(&request.path)));
        }
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
            Package::Local(path) | Package::Dev(path) => path,
            Package::Downloaded { staged, .. } => &staged.path,
        }
    }

    pub fn warnings(&self) -> Vec<WarningInfo> {
        match self {
            Package::Local(_) | Package::Dev(_) => Vec::new(),
            Package::Downloaded { staged, .. } => staged.warnings.clone(),
        }
    }

    pub fn token(&self) -> Option<&str> {
        match self {
            Package::Local(_) | Package::Dev(_) => None,
            Package::Downloaded { token, .. } => Some(token),
        }
    }
}

fn both() -> ErrorInfo {
    ErrorInfo::PluginInvalid {
        detail: "give a path or a link, not both".to_string(),
    }
}
