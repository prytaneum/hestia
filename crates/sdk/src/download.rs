//! Have the daemon fetch a file to disk.
//!
//! The plugin needs `network:connect`, and the URL and every redirect it
//! follows must be on hosts its grant allows. The daemon does the transfer: it
//! verifies the checksum before the file appears, serves a repeat from its
//! cache, shows progress in the running command or build, and stops when that
//! is cancelled. The plugin never holds the body.
//!
//! ```no_run
//! use hestia::download::{download, Expect};
//!
//! let sha1 = "32a22d349740b9065493b697ae61279f4f85aea2";
//! let file = download(
//!     "https://files.example.com/tool.jar",
//!     Expect::Sha1(sha1),
//!     "/build/tool.jar",
//! )?;
//! hestia::log::info(&format!("fetched {} bytes", file.size));
//! # Ok::<(), hestia::Error>(())
//! ```

use crate::bindings::hestia::plugin::download as host;
use crate::Error;

/// What the downloaded file must be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Expect<'a> {
    /// A SHA-1 as 40 hex characters.
    Sha1(&'a str),
    /// A SHA-256 as 64 hex characters.
    Sha256(&'a str),
    /// A SHA-512 as 128 hex characters.
    Sha512(&'a str),
    /// Nothing verifies the file, which the grant allows only from a host it
    /// lists as serving unhashed files; any other is [`Error::Denied`].
    Unhashed,
}

/// The file a download wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Downloaded {
    /// Its size in bytes.
    pub size: u64,
}

/// Fetches `url` to `dest`, a path under a writable mount of this call: `/data`,
/// or `/build` while a flavor's `build` runs. Its directory must exist; any
/// other path is [`Error::Denied`]. The file appears only once it is complete
/// and matches `expect`, so a failed download leaves nothing at `dest`.
///
/// A missing file is [`Error::NotFound`], any other failing status
/// [`Error::Upstream`], and no route [`Error::Offline`]. A file over 4 GiB is
/// refused as it streams.
pub fn download(url: &str, expect: Expect<'_>, dest: &str) -> Result<Downloaded, Error> {
    let checksum = |algorithm, digest: &str| host::Checksum {
        algorithm,
        hex: digest.to_string(),
    };
    let expect = match expect {
        Expect::Sha1(digest) => Some(checksum(host::Algorithm::Sha1, digest)),
        Expect::Sha256(digest) => Some(checksum(host::Algorithm::Sha256, digest)),
        Expect::Sha512(digest) => Some(checksum(host::Algorithm::Sha512, digest)),
        Expect::Unhashed => None,
    };
    let outcome = host::download(url, expect.as_ref(), dest)?;
    Ok(Downloaded { size: outcome.size })
}
