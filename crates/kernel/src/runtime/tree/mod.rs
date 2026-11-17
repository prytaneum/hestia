//! A program and everything it starts, ended together: a process group on
//! unix, a job object on Windows. Each platform module offers the same
//! `isolate`, `Tree::of`, `Tree::start` and `Tree::kill`.

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
pub(super) use unix::{isolate, Tree};
#[cfg(windows)]
pub(super) use windows::{isolate, Tree};
