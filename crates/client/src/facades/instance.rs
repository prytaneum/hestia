use ipc::errors::IpcError;
use proto::instance::{AddressPing, AddressPingParams};
use proto::instance::{
    InstanceCreate, InstanceCreateParams, InstanceDetails, InstanceInfo, InstanceInfoQuery,
    InstanceLaunch, InstanceLaunchDone, InstanceLaunchParams, InstanceList, InstanceRecent,
    InstanceRef, InstanceServerEdit, InstanceServerEditParams, InstanceServerRef,
    InstanceServerRemove, InstanceServers, InstanceServersArrange, InstanceServersArrangeParams,
    InstanceServersWriteResult, InstanceUpdate, InstanceUpdateDone, InstanceUpdateParams,
    InstanceWorlds, QuickPlay, RecentPlay, Replace, ServerEntry, WorldInfo,
};
use proto::minecraft::{ConfigEntry, ProvisionProgress};
use proto::server::ServerPingResult;

use crate::session::{job_id, Session};

pub struct Instance<'a> {
    pub(crate) session: &'a Session,
}

/// One launch's options. A struct rather than a parameter list — `new_session`
/// and `offline` are adjacent bools a caller could otherwise swap silently.
#[derive(Default)]
pub struct LaunchOptions<'a> {
    pub instance: &'a str,
    /// Account name or uuid; empty picks the sole signed-in account.
    pub account: &'a str,
    /// Launch another session even when one is already running.
    pub new_session: bool,
    /// Join a world or server on start instead of opening to the title screen.
    pub quick_play: Option<QuickPlay>,
    /// Skip Microsoft entirely and run unauthenticated.
    pub offline: bool,
    /// Stop these sessions first and launch once they have exited.
    pub replace: Option<Replace>,
}

impl Instance<'_> {
    /// Create an instance record (the profile is resolved upstream, so this can
    /// take a little while; files are materialised at launch).
    pub async fn create(
        &self,
        name: &str,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
        config: Vec<ConfigEntry>,
    ) -> Result<InstanceInfo, IpcError> {
        let params = InstanceCreateParams {
            name: name.to_string(),
            flavor: flavor.to_string(),
            version: version.to_string(),
            loader_version,
            config,
        };
        self.session.call::<InstanceCreate>(&params).await
    }

    /// Move a stopped instance to another version, blocking until the daemon
    /// reports done or error (the new files materialise at the next launch;
    /// a modpack moves with it per `params.pack`). Nothing is backed up first
    /// — instances have no backups. `params.allow_downgrade` asserts the user
    /// confirmed a downgrade; the job id is filled in here.
    pub async fn update(
        &self,
        mut params: InstanceUpdateParams,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<InstanceUpdateDone, IpcError> {
        let id = job_id("instance-update");
        params.id = id.clone();
        self.session
            .run_job::<InstanceUpdate>(&id, &params, on_progress)
            .await
    }

    pub async fn list(&self) -> Result<Vec<InstanceInfo>, IpcError> {
        self.session.call::<InstanceList>(&proto::Empty {}).await
    }

    /// The instance's static, informational view (locations + disk footprint).
    pub async fn info(&self, instance: &str) -> Result<InstanceDetails, IpcError> {
        self.session
            .call::<InstanceInfoQuery>(&instance_ref(instance))
            .await
    }

    /// The instance's save worlds, each described from its own `level.dat` —
    /// the worlds a datapack can install into.
    pub async fn worlds(&self, instance: &str) -> Result<Vec<WorldInfo>, IpcError> {
        self.session
            .call::<InstanceWorlds>(&instance_ref(instance))
            .await
    }

    /// What was joined lately across every instance, most recent first — each
    /// one a launch of `instance` with `target` as its quick play.
    pub async fn recent(&self) -> Result<Vec<RecentPlay>, IpcError> {
        self.session.call::<InstanceRecent>(&proto::Empty {}).await
    }

    /// The instance's multiplayer list (`servers.dat`), in the order the game
    /// shows it.
    pub async fn servers(&self, instance: &str) -> Result<Vec<ServerEntry>, IpcError> {
        self.session
            .call::<InstanceServers>(&instance_ref(instance))
            .await
    }

    /// Add an entry to the multiplayer list, or rewrite the one `target` names
    /// (empty adds). The running game owns the file, so the result carries a
    /// warning when a session is open to say the edit will be overwritten.
    pub async fn server_edit(
        &self,
        instance: &str,
        target: &str,
        name: &str,
        address: &str,
        accept_textures: bool,
    ) -> Result<InstanceServersWriteResult, IpcError> {
        self.session
            .call::<InstanceServerEdit>(&InstanceServerEditParams {
                instance: instance.to_string(),
                server: target.to_string(),
                name: name.to_string(),
                address: address.to_string(),
                accept_textures,
            })
            .await
    }

    pub async fn server_remove(
        &self,
        instance: &str,
        target: &str,
    ) -> Result<InstanceServersWriteResult, IpcError> {
        self.session
            .call::<InstanceServerRemove>(&InstanceServerRef {
                instance: instance.to_string(),
                server: target.to_string(),
            })
            .await
    }

    /// Rewrite the order of the multiplayer list. `order` names every visible
    /// entry — the rows the caller listed — in the order they should sit.
    pub async fn servers_arrange(
        &self,
        instance: &str,
        order: Vec<String>,
    ) -> Result<InstanceServersWriteResult, IpcError> {
        self.session
            .call::<InstanceServersArrange>(&InstanceServersArrangeParams {
                instance: instance.to_string(),
                order,
            })
            .await
    }

    /// Status of a multiplayer address, over the Server List Ping — what the
    /// in-game list shows beside each entry.
    pub async fn ping_address(&self, address: &str) -> Result<ServerPingResult, IpcError> {
        self.session
            .call::<AddressPing>(&AddressPingParams {
                address: address.to_string(),
            })
            .await
    }

    /// Launch an instance, blocking until the game process has spawned (or the
    /// preparation failed) and forwarding each progress event to `on_progress`.
    /// Returns the supervised process id and pid, plus anything the preparation
    /// could not do properly (a sync target left unshared, for instance).
    pub async fn launch(
        &self,
        options: LaunchOptions<'_>,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<InstanceLaunchDone, IpcError> {
        let id = job_id("instance-launch");
        let params = InstanceLaunchParams {
            instance: options.instance.to_string(),
            account: options.account.to_string(),
            id: id.clone(),
            new_session: options.new_session,
            quick_play: options.quick_play,
            offline: options.offline,
            replace: options.replace,
        };
        self.session
            .run_job::<InstanceLaunch>(&id, &params, on_progress)
            .await
    }
}

fn instance_ref(instance: &str) -> InstanceRef {
    InstanceRef {
        instance: instance.to_string(),
    }
}
