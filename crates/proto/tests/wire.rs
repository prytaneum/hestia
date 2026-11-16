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
fn an_error_hint_is_derived_from_the_variant_and_never_sent() {
    use proto::error::ErrorInfo::*;
    use proto::error::Field;

    let with_hint = [
        SignInRequired,
        SessionExpired {
            reference: "steve".into(),
        },
        MultiSessionDisabled { name: "smp".into() },
        OfflineMode,
    ];
    let without_hint = [
        FieldRequired { field: Field::Name },
        EntryRunning {
            entry: proto::error::EntryKind::Server,
            name: "smp".into(),
        },
        Internal {
            detail: "boom".into(),
        },
    ];
    for info in &with_hint {
        let hint = info.hint();
        assert!(
            hint.as_deref().is_some_and(|text| !text.is_empty()),
            "{info:?}"
        );
    }
    for info in &without_hint {
        assert_eq!(info.hint(), None, "{info:?}");
    }
}
