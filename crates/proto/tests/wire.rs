//! Wire-format guards: the JSON shapes the daemon and CLI marshal through must
//! match the C++ `kFields` codec.

use proto::cache::CacheEntry;
use proto::download::{Checksum, HashAlgorithm};
use proto::java::{JavaInstallPhase, JavaInstallProgress};
use proto::Empty;
use serde_json::json;

#[test]
fn empty_is_an_object() {
    assert_eq!(serde_json::to_value(Empty {}).unwrap(), json!({}));
    let _: Empty = serde_json::from_value(json!({})).unwrap();
}

#[test]
fn hash_algorithm_is_lowercase_string() {
    assert_eq!(
        serde_json::to_value(HashAlgorithm::Sha1).unwrap(),
        json!("sha1")
    );
    assert_eq!(
        serde_json::to_value(HashAlgorithm::Sha256).unwrap(),
        json!("sha256")
    );
    let a: HashAlgorithm = serde_json::from_value(json!("sha256")).unwrap();
    assert_eq!(a, HashAlgorithm::Sha256);
}

#[test]
fn unknown_hash_algorithm_is_rejected() {
    assert!(serde_json::from_value::<HashAlgorithm>(json!("md5")).is_err());
}

#[test]
fn cache_entry_flattens_checksum() {
    let entry = CacheEntry {
        checksum: Checksum {
            algorithm: HashAlgorithm::Sha256,
            hex: "ab".repeat(32),
        },
        size: 42,
    };
    let v = serde_json::to_value(&entry).unwrap();
    assert_eq!(v["algorithm"], json!("sha256"));
    assert_eq!(v["hex"], json!("ab".repeat(32)));
    assert_eq!(v["size"], json!(42));
    assert!(
        v.get("checksum").is_none(),
        "checksum must flatten, not nest"
    );
}

#[test]
fn java_install_progress_phase_is_lowercase() {
    let p = JavaInstallProgress {
        phase: JavaInstallPhase::Downloading,
        current: 10,
        total: 100,
    };
    let v = serde_json::to_value(&p).unwrap();
    assert_eq!(v["phase"], json!("downloading"));
    assert_eq!(v["current"], json!(10));
}

#[test]
fn every_job_reports_on_the_job_topic() {
    use proto::job::JobEvent;
    use proto::Topic;

    assert_eq!(JobEvent::<JavaInstallProgress, ()>::TOPIC, "job");
}

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
