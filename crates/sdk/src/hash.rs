//! Digests for checking downloads and naming content: SHA-1, SHA-256 and
//! SHA-512, as lowercase hex.
//!
//! ```
//! assert_eq!(
//!     hestia::hash::sha1(b"abc"),
//!     "a9993e364706816aba3e25717850c26c9cd0d89d"
//! );
//! ```

use std::fs::File;
use std::io::Read;
use std::path::Path;

use sha1::Sha1;
use sha2::digest::Digest;
use sha2::{Sha256, Sha512};

use crate::Error;

const CHUNK: usize = 64 * 1024;

/// The digest to compute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    Sha1,
    Sha256,
    Sha512,
}

impl Algorithm {
    /// The digest of `bytes` as lowercase hex.
    pub fn hash(self, bytes: &[u8]) -> String {
        match self {
            Algorithm::Sha1 => digest::<Sha1>(bytes),
            Algorithm::Sha256 => digest::<Sha256>(bytes),
            Algorithm::Sha512 => digest::<Sha512>(bytes),
        }
    }

    /// The digest of everything `reader` yields, read in chunks.
    pub fn hash_reader(self, reader: impl Read) -> Result<String, Error> {
        match self {
            Algorithm::Sha1 => digest_reader::<Sha1>(reader),
            Algorithm::Sha256 => digest_reader::<Sha256>(reader),
            Algorithm::Sha512 => digest_reader::<Sha512>(reader),
        }
    }
}

/// The SHA-1 of `bytes` as 40 lowercase hex characters.
pub fn sha1(bytes: &[u8]) -> String {
    Algorithm::Sha1.hash(bytes)
}

/// The SHA-256 of `bytes` as 64 lowercase hex characters.
pub fn sha256(bytes: &[u8]) -> String {
    Algorithm::Sha256.hash(bytes)
}

/// The SHA-512 of `bytes` as 128 lowercase hex characters.
pub fn sha512(bytes: &[u8]) -> String {
    Algorithm::Sha512.hash(bytes)
}

/// The digest of a file, read in chunks so a large one is never held whole. The
/// plugin can reach only the files its mounts hold, such as its `data/`
/// directory; a missing file is [`Error::Io`].
///
/// ```no_run
/// use hestia::hash::{file, Algorithm};
///
/// let sum = file("data/archive.zip", Algorithm::Sha256)?;
/// # Ok::<(), hestia::Error>(())
/// ```
pub fn file(path: impl AsRef<Path>, algorithm: Algorithm) -> Result<String, Error> {
    algorithm.hash_reader(File::open(path)?)
}

fn digest<D: Digest>(bytes: &[u8]) -> String {
    hex(&D::digest(bytes))
}

fn digest_reader<D: Digest>(mut reader: impl Read) -> Result<String, Error> {
    let mut hasher = D::new();
    let mut buffer = vec![0u8; CHUNK];
    loop {
        match reader.read(&mut buffer)? {
            0 => return Ok(hex(&hasher.finalize())),
            read => hasher.update(&buffer[..read]),
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_digests() {
        assert_eq!(sha1(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(
            sha256(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha512(b"abc"),
            "ddaf35a193617abacc417349ae20413112e6fa4e89a97ea20a9eeee64b55d39a\
             2192992a274fc1a836ba3c23a3feebbd454d4423643ce80e2a9ac94fa54ca49f"
        );
    }

    #[test]
    fn a_reader_hashes_like_the_bytes_across_chunks() {
        let bytes = vec![7u8; CHUNK * 2 + 5];
        assert_eq!(
            Algorithm::Sha256.hash_reader(bytes.as_slice()).unwrap(),
            sha256(&bytes)
        );
    }
}
