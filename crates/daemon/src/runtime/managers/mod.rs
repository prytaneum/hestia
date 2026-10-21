//! Worker jobs that run blocking engine work off the request path: an install
//! or download answers immediately while progress and the terminal outcome are
//! published through the event hub. One [`Jobs`] value owns them all; each
//! family adds its `start_*` function in its own file.

mod backup;
mod content;
mod instance;
mod java;
mod job;
mod modpack;
mod plugin;
mod server;
mod transfer;
mod update;

pub use backup::BackupJob;
pub use content::{ContentJob, Entry as JobEntry};
pub use instance::LaunchOrder;
pub(crate) use job::generate_id as generate_job_id;
pub use job::Jobs;
pub use modpack::ModpackJob;
pub use transfer::ExportOrder;
