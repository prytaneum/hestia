//! The dev override is only ever reached with an overridden endpoint beside it,
//! so a stray `HESTIA_UPDATE_PUBKEY` in someone's shell cannot quietly change
//! what their build trusts.

#[test]
fn a_pubkey_override_alone_changes_nothing() {
    std::env::set_var("HESTIA_UPDATE_PUBKEY", "a-key-nobody-asked-for");
    let keys: Vec<_> = common::app::update_pubkeys().collect();
    std::env::remove_var("HESTIA_UPDATE_PUBKEY");
    let compiled: Vec<_> = [common::app::UPDATE_PUBKEY, common::app::UPDATE_PUBKEY_NEXT]
        .into_iter()
        .filter(|key| !key.is_empty())
        .collect();
    assert_eq!(keys, compiled);
}
