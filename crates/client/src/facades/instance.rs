use std::time::Duration;

use ipc::errors::IpcError;
use proto::content::{
    ContentAddSpec, ContentFailure, ContentKind, ContentMatch, ContentUpdate, InstalledContent,
    InstanceContentAdd, InstanceContentAddParams, InstanceContentCheckUpdates,
    InstanceContentCheckUpdatesParams, InstanceContentEnable, InstanceContentEnableParams,
    InstanceContentList, InstanceContentListParams, InstanceContentMatches,
    InstanceContentMatchesParams, InstanceContentRemove, InstanceContentRemoveParams,
    InstanceContentSetVersion, InstanceContentSetVersionParams, InstanceContentUpdate,
    InstanceContentUpdateParams, UntrackedFile,
};
use proto::instance::{AddressPing, AddressPingParams};
use proto::instance::{
    InstanceConfigGet, InstanceConfigGetParams, InstanceConfigList, InstanceConfigSet,
    InstanceConfigSetParams, InstanceCreate, InstanceCreateParams, InstanceDetails,
    InstanceFlavors, InstanceInfo, InstanceInfoQuery, InstanceLaunch, InstanceLaunchDoneEvent,
    InstanceLaunchParams, InstanceList, InstanceLoaders, InstanceLogs, InstanceLogsParams,
    InstanceRecent, InstanceRef, InstanceRemove, InstanceRename, InstanceRenameParams,
    InstanceResolve, InstanceServerEdit, InstanceServerEditParams, InstanceServerRef,
    InstanceServerRemove, InstanceServers, InstanceServersArrange, InstanceServersArrangeParams,
    InstanceServersWriteResult, InstanceStop, InstanceStopParams, InstanceUpdate,
    InstanceUpdateParams, InstanceVersions, InstanceWorlds, QuickPlay, RecentPlay, ServerEntry,
    WorldInfo,
};
use proto::minecraft::{
    ConfigEntry, Flavor, GameVersion, InstanceProfile, LoadersParams, ProvisionProgress,
    ResolveParams, VersionsParams,
};
use proto::process::ProcessLogLine;
use proto::server::ServerPingResult;

use crate::facades::jobs::{forward, run_content_job};
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
}

impl Instance<'_> {
    pub async fn flavors(&self) -> Result<Vec<Flavor>, IpcError> {
        Ok(self
            .session
            .call::<InstanceFlavors>(&proto::Empty {})
            .await?
            .flavors)
    }

    pub async fn versions(&self, flavor: &str) -> Result<Vec<GameVersion>, IpcError> {
        let params = VersionsParams {
            flavor: flavor.to_string(),
        };
        Ok(self
            .session
            .call::<InstanceVersions>(&params)
            .await?
            .versions)
    }

    pub async fn resolve(
        &self,
        flavor: &str,
        version: &str,
        loader_version: Option<String>,
    ) -> Result<InstanceProfile, IpcError> {
        let params = ResolveParams {
            flavor: flavor.to_string(),
            version: version.to_string(),
            loader_version,
        };
        self.session.call::<InstanceResolve>(&params).await
    }

    pub async fn loaders(&self, flavor: &str, version: &str) -> Result<Vec<String>, IpcError> {
        let params = LoadersParams {
            flavor: flavor.to_string(),
            version: version.to_string(),
        };
        Ok(self.session.call::<InstanceLoaders>(&params).await?.loaders)
    }

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
        Ok(self
            .session
            .call_with_timeout::<InstanceCreate>(&params, Duration::from_secs(60))
            .await?
            .instance)
    }

    /// Move a stopped instance to another version (the profile is resolved
    /// upstream; the new files materialise at the next launch). Nothing is
    /// backed up first — instances have no backups. `allow_downgrade` asserts
    /// the user confirmed a downgrade.
    pub async fn update(
        &self,
        instance: &str,
        version: &str,
        loader_version: Option<String>,
        allow_downgrade: bool,
    ) -> Result<InstanceInfo, IpcError> {
        let params = InstanceUpdateParams {
            instance: instance.to_string(),
            version: version.to_string(),
            loader_version,
            allow_downgrade,
        };
        Ok(self
            .session
            .call_with_timeout::<InstanceUpdate>(&params, Duration::from_secs(10 * 60))
            .await?
            .instance)
    }

    pub async fn list(&self) -> Result<Vec<InstanceInfo>, IpcError> {
        Ok(self
            .session
            .call::<InstanceList>(&proto::Empty {})
            .await?
            .instances)
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
        Ok(self
            .session
            .call::<InstanceWorlds>(&instance_ref(instance))
            .await?
            .worlds)
    }

    /// What was joined lately across every instance, most recent first — each
    /// one a launch of `instance` with `target` as its quick play.
    pub async fn recent(&self) -> Result<Vec<RecentPlay>, IpcError> {
        Ok(self
            .session
            .call::<InstanceRecent>(&proto::Empty {})
            .await?
            .plays)
    }

    /// The instance's multiplayer list (`servers.dat`), in the order the game
    /// shows it.
    pub async fn servers(&self, instance: &str) -> Result<Vec<ServerEntry>, IpcError> {
        Ok(self
            .session
            .call::<InstanceServers>(&instance_ref(instance))
            .await?
            .servers)
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

    pub async fn remove(&self, instance: &str) -> Result<(), IpcError> {
        self.session
            .call::<InstanceRemove>(&instance_ref(instance))
            .await?;
        Ok(())
    }

    /// Rename a stopped instance; the id (directory slug) is re-derived from
    /// the new name. Returns the updated record, whose `id` and `name` reflect
    /// the rename.
    pub async fn rename(&self, instance: &str, name: &str) -> Result<InstanceInfo, IpcError> {
        let params = InstanceRenameParams {
            instance: instance.to_string(),
            name: name.to_string(),
        };
        self.session.call::<InstanceRename>(&params).await
    }

    /// Stop a session (when `session` is set) or all of the instance's sessions.
    pub async fn stop(&self, instance: &str, session: Option<String>) -> Result<(), IpcError> {
        let params = InstanceStopParams {
            instance: instance.to_string(),
            session,
        };
        self.session.call::<InstanceStop>(&params).await?;
        Ok(())
    }

    pub async fn logs(
        &self,
        instance: &str,
        session: Option<String>,
        tail: Option<usize>,
    ) -> Result<Vec<ProcessLogLine>, IpcError> {
        let params = InstanceLogsParams {
            instance: instance.to_string(),
            session,
            tail,
        };
        Ok(self.session.call::<InstanceLogs>(&params).await?.lines)
    }

    /// Read one JVM setting; `None` when it is not set (a `not_found` from the
    /// daemon).
    pub async fn config_get(&self, instance: &str, key: &str) -> Result<Option<String>, IpcError> {
        let params = InstanceConfigGetParams {
            instance: instance.to_string(),
            key: key.to_string(),
        };
        Ok(self
            .session
            .try_call::<InstanceConfigGet>(&params)
            .await?
            .map(|r| r.value))
    }

    pub async fn config_set(&self, instance: &str, key: &str, value: &str) -> Result<(), IpcError> {
        let params = InstanceConfigSetParams {
            instance: instance.to_string(),
            key: key.to_string(),
            value: value.to_string(),
        };
        self.session.call::<InstanceConfigSet>(&params).await?;
        Ok(())
    }

    pub async fn config_list(&self, instance: &str) -> Result<Vec<ConfigEntry>, IpcError> {
        Ok(self
            .session
            .call::<InstanceConfigList>(&instance_ref(instance))
            .await?
            .entries)
    }

    /// Launch an instance, blocking until the game process has spawned (or the
    /// preparation failed) and forwarding each progress event to `on_progress`.
    /// Returns the supervised process id and pid, plus anything the preparation
    /// could not do properly (a sync target left unshared, for instance).
    pub async fn launch(
        &self,
        options: LaunchOptions<'_>,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<InstanceLaunchDoneEvent, IpcError> {
        let id = job_id("instance-launch");
        let session = self.session;
        let params = InstanceLaunchParams {
            instance: options.instance.to_string(),
            account: options.account.to_string(),
            id: id.clone(),
            new_session: options.new_session,
            quick_play: options.quick_play,
            offline: options.offline,
        };
        let payload = self
            .session
            .run_job(
                &id,
                "instance.launch.done",
                "instance.launch.error",
                forward(on_progress),
                move || async move { session.call::<InstanceLaunch>(&params).await.map(|_| ()) },
            )
            .await?;

        serde_json::from_value(payload).map_err(|e| IpcError::Malformed(e.to_string()))
    }

    /// Install a batch of content into an instance, blocking until the daemon
    /// reports done or error and forwarding each progress event to
    /// `on_progress`. Returns everything installed (items plus required
    /// dependencies) and, per item that could not be installed, a failure.
    pub async fn content_add(
        &self,
        instance: &str,
        spec: ContentAddSpec,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>), IpcError> {
        let id = job_id("instance-content-add");
        let params = InstanceContentAddParams {
            instance: instance.to_string(),
            spec,
            id: id.clone(),
        };
        let session = self.session;
        run_content_job(session, &id, on_progress, move || async move {
            session
                .call::<InstanceContentAdd>(&params)
                .await
                .map(|_| ())
        })
        .await
    }

    pub async fn content_list(
        &self,
        instance: &str,
        kind: ContentKind,
    ) -> Result<(Vec<InstalledContent>, Vec<UntrackedFile>), IpcError> {
        let params = InstanceContentListParams {
            instance: instance.to_string(),
            kind,
        };
        let result = self.session.call::<InstanceContentList>(&params).await?;
        Ok((result.items, result.untracked))
    }

    /// Uninstall the named items and every mirror of them, in one call. A
    /// non-empty `worlds` instead narrows a datapack to the save worlds it
    /// keeps loading in.
    pub async fn content_remove(
        &self,
        instance: &str,
        kind: ContentKind,
        items: &[String],
        worlds: &[String],
    ) -> Result<(), IpcError> {
        let params = InstanceContentRemoveParams {
            instance: instance.to_string(),
            kind,
            items: items.to_vec(),
            worlds: worlds.to_vec(),
        };
        self.session.call::<InstanceContentRemove>(&params).await?;
        Ok(())
    }

    /// Update platform-sourced content to its newest compatible version — the
    /// named items in one job, or every item of the kind when `items` is empty,
    /// with a failure per item that could not move. A non-empty `path` instead
    /// updates the one named local item from that file.
    pub async fn content_update(
        &self,
        instance: &str,
        kind: ContentKind,
        items: &[String],
        path: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<(Vec<InstalledContent>, Vec<ContentFailure>), IpcError> {
        let id = job_id("instance-content-update");
        let params = InstanceContentUpdateParams {
            instance: instance.to_string(),
            kind,
            items: items.to_vec(),
            path: path.to_string(),
            id: id.clone(),
        };
        let session = self.session;
        run_content_job(session, &id, on_progress, move || async move {
            session
                .call::<InstanceContentUpdate>(&params)
                .await
                .map(|_| ())
        })
        .await
    }

    /// Enable or disable one installed item; a non-empty `worlds` scopes a
    /// datapack toggle to those save worlds.
    pub async fn content_enable(
        &self,
        instance: &str,
        kind: ContentKind,
        item: &str,
        enabled: bool,
        worlds: &[String],
    ) -> Result<(), IpcError> {
        let params = InstanceContentEnableParams {
            instance: instance.to_string(),
            kind,
            item: item.to_string(),
            enabled,
            worlds: worlds.to_vec(),
        };
        self.session.call::<InstanceContentEnable>(&params).await?;
        Ok(())
    }

    /// Which platform-sourced items of the kind have a newer compatible version.
    pub async fn content_check_updates(
        &self,
        instance: &str,
        kind: ContentKind,
    ) -> Result<Vec<ContentUpdate>, IpcError> {
        let params = InstanceContentCheckUpdatesParams {
            instance: instance.to_string(),
            kind,
        };
        let result = self
            .session
            .call_with_timeout::<InstanceContentCheckUpdates>(&params, Duration::from_secs(120))
            .await?;
        Ok(result.updates)
    }

    pub async fn content_matches(
        &self,
        instance: &str,
        kind: ContentKind,
        item: &str,
    ) -> Result<Vec<ContentMatch>, IpcError> {
        let params = InstanceContentMatchesParams {
            instance: instance.to_string(),
            kind,
            item: item.to_string(),
        };
        let result = self
            .session
            .call_with_timeout::<InstanceContentMatches>(&params, Duration::from_secs(120))
            .await?;
        Ok(result.matches)
    }

    /// Re-pin one item to a specific published `version` (id or number).
    pub async fn content_set_version(
        &self,
        instance: &str,
        kind: ContentKind,
        item: &str,
        version: &str,
        source: &str,
        on_progress: impl Fn(&ProvisionProgress) + Send + Sync + 'static,
    ) -> Result<Vec<InstalledContent>, IpcError> {
        let id = job_id("instance-content-set-version");
        let params = InstanceContentSetVersionParams {
            instance: instance.to_string(),
            kind,
            item: item.to_string(),
            version: version.to_string(),
            source: source.to_string(),
            id: id.clone(),
        };
        let session = self.session;
        run_content_job(session, &id, on_progress, move || async move {
            session
                .call::<InstanceContentSetVersion>(&params)
                .await
                .map(|_| ())
        })
        .await
        .map(|(items, _)| items)
    }
}

fn instance_ref(instance: &str) -> InstanceRef {
    InstanceRef {
        instance: instance.to_string(),
    }
}
