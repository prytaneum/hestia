use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    Sha1,
    Sha256,
}

impl HashAlgorithm {
    /// Length of the algorithm's digest in hex characters (40 / 64).
    pub fn hex_digest_length(self) -> usize {
        match self {
            HashAlgorithm::Sha1 => 40,
            HashAlgorithm::Sha256 => 64,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            HashAlgorithm::Sha1 => "sha1",
            HashAlgorithm::Sha256 => "sha256",
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(rename_all = "camelCase")]
pub struct Checksum {
    pub algorithm: HashAlgorithm,
    #[serde(default)]
    pub hex: String,
}

impl Checksum {
    /// Well-formed when `hex` is exactly the algorithm's digest length and holds
    /// only hex characters — one definition every caller validates against.
    pub fn is_valid(&self) -> bool {
        self.hex.len() == self.algorithm.hex_digest_length()
            && self.hex.bytes().all(|b| b.is_ascii_hexdigit())
    }
}

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export, optional_fields))]
#[serde(default, rename_all = "camelCase")]
pub struct DownloadProgress {
    pub downloaded: u64,
    pub total: u64,
}
