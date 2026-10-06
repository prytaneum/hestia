//! Wire-compat golden tests. Each fixture under `tests/golden/` is a canonical
//! JSON wire form for one `proto` type; the test decodes it into that type and
//! re-encodes it, asserting the round-tripped value equals the fixture. A field
//! renamed, dropped, added, or an enum string changed makes this fail — the
//! guard that a `proto` type cannot silently drift from the shape on the socket.
//!
//! (The original plan captured these from the running C++ daemon; after the
//! all-Rust cutover there is no C++ oracle, so the fixtures are maintained by
//! hand as the authoritative wire contract.)

use proto::accounts::{Account, AccountLoginBeginResult};
use proto::app::AppInfoResult;
use proto::backup::BackupInfo;
use proto::cache::{CacheEntry, CacheInfoResult};
use proto::content::{
    ContentAddSpec, ContentDoneEvent, ContentErrorEvent, ContentProgressEvent, ContentProject,
    ContentVersion, InstalledContent, SearchQuery, ServerContentAddParams,
};
use proto::daemon::{DaemonStatusResult, DaemonStopParams};
use proto::download::DownloadSpec;
use proto::instance::{InstanceInfo, InstanceLaunchParams, RecentPlay, ServerEntry};
use proto::java::{JavaInstallProgress, JavaRuntime};
use proto::minecraft::ProvisionProgress;
use proto::process::{
    ProcessExitEvent, ProcessInfo, ProcessMetricsEvent, ProcessOutputEvent, ProcessSpec,
};
use proto::server::{ServerInfo, ServerPingResult};
use proto::skins::{Cape, Skin};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// Decode `fixture` into `T`, re-encode it, and assert the value is unchanged.
/// Object key order and whitespace are irrelevant (compared as `Value`); a
/// changed field name, set, or enum string is not.
fn round_trip<T: Serialize + DeserializeOwned>(name: &str, fixture: &str) {
    let expected: Value = serde_json::from_str(fixture)
        .unwrap_or_else(|e| panic!("{name}: fixture is not JSON: {e}"));
    let decoded: T = serde_json::from_value(expected.clone())
        .unwrap_or_else(|e| panic!("{name}: fixture does not decode into the type: {e}"));
    let reencoded = serde_json::to_value(&decoded).unwrap();
    assert_eq!(
        reencoded, expected,
        "{name}: re-encoded wire form drifted from the golden fixture"
    );
}

macro_rules! golden {
    ($test:ident, $ty:ty, $file:literal) => {
        #[test]
        fn $test() {
            round_trip::<$ty>($file, include_str!(concat!("golden/", $file)));
        }
    };
}

golden!(app_info_result, AppInfoResult, "app_info_result.json");
golden!(
    daemon_status_result,
    DaemonStatusResult,
    "daemon_status_result.json"
);
golden!(cache_entry, CacheEntry, "cache_entry.json");
golden!(cache_info_result, CacheInfoResult, "cache_info_result.json");
golden!(java_runtime, JavaRuntime, "java_runtime.json");
golden!(
    java_install_progress,
    JavaInstallProgress,
    "java_install_progress.json"
);
golden!(download_spec, DownloadSpec, "download_spec.json");
golden!(account, Account, "account.json");
golden!(
    account_login_begin_result,
    AccountLoginBeginResult,
    "account_login_begin_result.json"
);
golden!(process_spec, ProcessSpec, "process_spec.json");
golden!(
    process_spec_log_file,
    ProcessSpec,
    "process_spec_log_file.json"
);
golden!(
    daemon_stop_params,
    DaemonStopParams,
    "daemon_stop_params.json"
);
golden!(process_info, ProcessInfo, "process_info.json");
golden!(
    process_output_event,
    ProcessOutputEvent,
    "process_output_event.json"
);
golden!(
    process_exit_event,
    ProcessExitEvent,
    "process_exit_event.json"
);
golden!(server_info, ServerInfo, "server_info.json");
golden!(instance_info, InstanceInfo, "instance_info.json");
golden!(
    instance_launch_params,
    InstanceLaunchParams,
    "instance_launch_params.json"
);
golden!(
    instance_launch_quick_play,
    InstanceLaunchParams,
    "instance_launch_quick_play.json"
);
golden!(server_entry, ServerEntry, "server_entry.json");
golden!(recent_play, RecentPlay, "recent_play.json");
golden!(backup_info, BackupInfo, "backup_info.json");
golden!(
    provision_progress,
    ProvisionProgress,
    "provision_progress.json"
);
golden!(content_add_spec, ContentAddSpec, "content_add_spec.json");
golden!(
    server_content_add_params,
    ServerContentAddParams,
    "server_content_add_params.json"
);
golden!(
    installed_content,
    InstalledContent,
    "installed_content.json"
);
golden!(
    content_progress_event,
    ContentProgressEvent,
    "content_progress_event.json"
);
golden!(
    content_done_event,
    ContentDoneEvent,
    "content_done_event.json"
);
golden!(
    content_error_event,
    ContentErrorEvent,
    "content_error_event.json"
);
golden!(search_query, SearchQuery, "search_query.json");
golden!(content_version, ContentVersion, "content_version.json");
golden!(content_project, ContentProject, "content_project.json");
golden!(skin, Skin, "skin.json");
golden!(cape, Cape, "cape.json");
golden!(
    server_ping_result,
    ServerPingResult,
    "server_ping_result.json"
);
golden!(
    process_metrics_event,
    ProcessMetricsEvent,
    "process_metrics_event.json"
);
