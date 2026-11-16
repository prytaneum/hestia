use proto::error::{code, ErrorInfo};

use crate::runtime::GuestError;

/// The router's failure as the error a plugin handles. Plugin authors branch on
/// these, so each is chosen per variant, never by code alone.
pub(crate) fn plugin_error(info: ErrorInfo) -> GuestError {
    use ErrorInfo::*;
    let text = info.to_string();
    match info {
        PluginDisabled { .. } | UnsupportedOperation { .. } => GuestError::Unsupported,
        Offline { .. } | OfflineMode => GuestError::Offline,
        Upstream { .. } => GuestError::Upstream(text),
        EntryRunning { .. }
        | WorkRunning
        | PluginBusy { .. }
        | UpdateInProgress { .. }
        | ContentInProgress { .. }
        | BackupInProgress { .. }
        | ExportInProgress { .. }
        | CreateInProgress
        | JavaInstallInProgress { .. } => GuestError::Busy,
        SignInRequired | SessionExpired { .. } | JobCancelled { .. } => GuestError::Failed(text),
        other => match other.code() {
            code::NOT_FOUND => GuestError::NotFound(text),
            code::BAD_REQUEST => GuestError::Invalid(text),
            _ => GuestError::Failed(text),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_router_failure_reaches_the_plugin_as_the_error_it_branches_on() {
        let cases = [
            (
                ErrorInfo::PluginDisabled { plugin: "p".into() },
                "unsupported",
            ),
            (ErrorInfo::OfflineMode, "offline"),
            (ErrorInfo::Offline { service: None }, "offline"),
            (ErrorInfo::WorkRunning, "busy"),
            (ErrorInfo::CreateInProgress, "busy"),
            (
                ErrorInfo::PluginNotFound { plugin: "p".into() },
                "not-found",
            ),
            (
                ErrorInfo::MalformedRequest { detail: "x".into() },
                "invalid",
            ),
            (ErrorInfo::SignInRequired, "failed"),
        ];
        for (info, expected) in cases {
            let kind = match plugin_error(info.clone()) {
                GuestError::Unsupported => "unsupported",
                GuestError::Offline => "offline",
                GuestError::Busy => "busy",
                GuestError::NotFound(_) => "not-found",
                GuestError::Invalid(_) => "invalid",
                GuestError::Failed(_) => "failed",
                other => panic!("{info:?} became {other:?}"),
            };
            assert_eq!(kind, expected, "{info:?}");
        }
    }
}
