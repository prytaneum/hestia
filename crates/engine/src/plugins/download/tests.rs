use proto::download::HashAlgorithm;

use super::*;

fn checksum(algorithm: HashAlgorithm, hex: &str) -> Checksum {
    Checksum {
        algorithm,
        hex: hex.into(),
    }
}

#[test]
fn a_checksum_is_checked_for_length_and_folded_to_lowercase() {
    let sha1 = "ABCDEF0123456789ABCDEF0123456789ABCDEF01";
    let folded = normalised(checksum(HashAlgorithm::Sha1, sha1)).unwrap();
    assert_eq!(folded.hex, sha1.to_ascii_lowercase());

    for bad in [
        checksum(HashAlgorithm::Sha1, "abc"),
        checksum(HashAlgorithm::Sha256, &"a".repeat(40)),
        checksum(HashAlgorithm::Sha512, &"z".repeat(128)),
    ] {
        assert!(matches!(normalised(bad), Err(GuestError::Invalid(_))));
    }
    assert!(normalised(checksum(HashAlgorithm::Sha512, &"a".repeat(128))).is_ok());
}

#[test]
fn a_logged_place_is_host_and_path_without_the_query() {
    assert_eq!(
        place("https://files.example.com/a/b.jar?token=secret#x"),
        "files.example.com/a/b.jar"
    );
    assert_eq!(place("not a url"), "");
}
