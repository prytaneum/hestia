//! What a plugin system builds on, kept apart from any runtime: the programs a
//! download or build may run, and the host matching that vets where a download
//! may come from.

mod host;
mod program;

pub use host::{is_network_host, network_matches, stays_inside};
pub use program::{
    is_program_env, Invocation, InvocationMismatch, JavaChoiceError, JavaProfile, JavaRange,
    Program, ProgramJava, ProgramPin,
};
