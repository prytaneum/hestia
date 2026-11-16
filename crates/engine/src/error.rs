//! The canonical `anyhow` → `ErrorInfo` projection, shared by the daemon's
//! request handlers and its job managers so a failure localizes identically
//! whether it returns from a request or arrives as a job's error event.

use proto::error::ErrorInfo;

use crate::ReauthRequired;

/// Project an engine `anyhow` failure onto the wire `ErrorInfo`: a typed
/// `ErrorInfo` the engine raised passes through untouched, a re-auth need
/// becomes `SessionExpired`, and anything else is an opaque `Internal` carrying
/// its English chain as `detail`.
pub fn error_info(error: anyhow::Error) -> ErrorInfo {
    match error.downcast::<ErrorInfo>() {
        Ok(info) => info,
        Err(error) => {
            if let Some(reauth) = error.downcast_ref::<ReauthRequired>() {
                return ErrorInfo::SessionExpired {
                    reference: reauth.reference.clone(),
                };
            }
            tracing::error!(error = ?error, "unhandled engine failure");
            ErrorInfo::Internal {
                detail: format!("{error:#}"),
            }
        }
    }
}

/// The refusal an archive's schema failure becomes: a document from a newer
/// build is unsupported, anything else is a malformed manifest.
pub(crate) fn archive_error(
    format: proto::error::ArchiveFormat,
    error: common::schema::SchemaError,
) -> ErrorInfo {
    use common::schema::SchemaError;
    use proto::error::ArchiveFault;
    match error {
        SchemaError::FromTheFuture { found, .. } => ErrorInfo::ArchiveTooNew {
            format,
            schema: found,
        },
        other => {
            tracing::warn!(error = %other, "an archive manifest cannot be decoded");
            ErrorInfo::ArchiveInvalid {
                format,
                reason: ArchiveFault::ManifestMalformed,
            }
        }
    }
}
