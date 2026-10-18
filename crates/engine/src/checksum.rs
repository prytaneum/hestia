//! Incremental SHA-1 / SHA-256 hashing. One definition every consumer (downloader, cache) verifies through.

use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use proto::download::HashAlgorithm;
use sha1::Sha1;
use sha2::{Digest, Sha256};

pub enum Hasher {
    Sha1(Sha1),
    Sha256(Sha256),
}

impl Hasher {
    pub fn new(algorithm: HashAlgorithm) -> Self {
        match algorithm {
            HashAlgorithm::Sha1 => Hasher::Sha1(Sha1::new()),
            HashAlgorithm::Sha256 => Hasher::Sha256(Sha256::new()),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Sha1(h) => h.update(data),
            Hasher::Sha256(h) => h.update(data),
        }
    }

    pub fn hex_digest(self) -> String {
        match self {
            Hasher::Sha1(h) => hex(&h.finalize()),
            Hasher::Sha256(h) => hex(&h.finalize()),
        }
    }
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// Streams the file, so a large archive is never held in memory. Generic over
/// the digest because an `.mrpack` index names its files by SHA-512, which the
/// wire's `HashAlgorithm` does not carry.
pub(crate) fn digest_file<D: Digest>(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = D::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hex(&hasher.finalize()))
}
