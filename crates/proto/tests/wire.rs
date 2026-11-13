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

#[test]
fn a_provider_owned_name_is_externally_tagged() {
    use proto::error::Service;
    use proto::provider::Provider;

    let open = serde_json::to_value(Service::Provider(Provider::new("fabric", "Fabric"))).unwrap();
    assert_eq!(
        open,
        serde_json::json!({"provider": {"id": "fabric", "name": "Fabric"}})
    );
    assert_eq!(serde_json::to_value(Service::Mojang).unwrap(), "mojang");
}

#[test]
fn a_rollback_event_is_camel_case_on_its_own_topic() {
    use proto::plugin::PluginRolledBackEvent;
    use proto::Topic;

    let event = PluginRolledBackEvent {
        id: "demo".into(),
        version: "2.0.0".into(),
        restored: "1.0.0".into(),
    };
    assert_eq!(PluginRolledBackEvent::TOPIC, "plugin.rolled_back");
    assert_eq!(
        serde_json::to_value(&event).unwrap(),
        serde_json::json!({"id": "demo", "version": "2.0.0", "restored": "1.0.0"})
    );
}
