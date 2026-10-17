use ipc::errors::IpcError;

use crate::session::{job_id, Session};

pub struct Java<'a> {
    pub(crate) session: &'a Session,
}

impl Java<'_> {
    pub async fn releases(&self) -> Result<Vec<proto::java::JavaRelease>, IpcError> {
        self.session
            .call::<proto::java::JavaReleases>(&proto::Empty {})
            .await
    }

    pub async fn list(&self) -> Result<Vec<proto::java::JavaRuntime>, IpcError> {
        self.session
            .call::<proto::java::JavaList>(&proto::Empty {})
            .await
    }

    pub async fn uninstall(&self, major: i32) -> Result<(), IpcError> {
        let params = proto::java::JavaUninstallParams { major };
        self.session
            .call::<proto::java::JavaUninstall>(&params)
            .await?;
        Ok(())
    }

    /// Install a runtime, blocking until the daemon reports done or error and
    /// forwarding each progress event to `on_progress`. Returns the registered
    /// runtime and whether it was already installed.
    ///
    /// `id` is the caller's job id — pass one to be able to cancel this install
    /// (`Client::cancel_job`); empty generates one, which is fine for a caller
    /// that will never cancel.
    pub async fn install(
        &self,
        major: i32,
        force: bool,
        id: &str,
        on_progress: impl Fn(&proto::java::JavaInstallProgress) + Send + Sync + 'static,
    ) -> Result<(proto::java::JavaRuntime, bool), IpcError> {
        use proto::java::{JavaInstall, JavaInstallParams};

        let id = match id.is_empty() {
            true => job_id("java-install"),
            false => id.to_string(),
        };
        let params = JavaInstallParams {
            major,
            id: id.clone(),
            force,
        };
        let done = self
            .session
            .run_job::<JavaInstall>(&id, &params, on_progress)
            .await?;
        Ok((done.runtime, done.already_installed))
    }
}
