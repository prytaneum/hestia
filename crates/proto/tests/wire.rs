//! A checksum is validated where it enters, before anything downloads against it.

use proto::download::{Checksum, HashAlgorithm};

#[test]
fn checksum_validation() {
    let good = Checksum {
        algorithm: HashAlgorithm::Sha1,
        hex: "a".repeat(40),
    };
    assert!(good.is_valid());
    let bad_len = Checksum {
        algorithm: HashAlgorithm::Sha1,
        hex: "a".repeat(39),
    };
    assert!(!bad_len.is_valid());
    let bad_char = Checksum {
        algorithm: HashAlgorithm::Sha1,
        hex: "z".repeat(40),
    };
    assert!(!bad_char.is_valid());
}
