//! The SDK's mock host against the host's permission table.

use hestia::{mock, secrets, Error};
use kernel::{Gate, Plugins};

#[test]
fn the_mock_gates_exactly_the_interfaces_the_table_does() {
    let home = tempfile::tempdir().unwrap();
    let plugins = Plugins::new(home.path(), Vec::new(), None).unwrap();
    let table = plugins.table();
    let listed: Vec<(&str, &str)> = include_str!("../../sdk/permissions.txt")
        .lines()
        .filter_map(|line| line.split_once(' '))
        .collect();
    assert!(listed
        .iter()
        .all(|(name, _)| table.interfaces().any(|interface| interface == *name)));
    for interface in table
        .interfaces()
        .filter(|name| name.starts_with("hestia:kernel/"))
    {
        let in_table = match table.gate(interface) {
            Gate::Needs(permission) => Some(permission.name),
            _ => None,
        };
        let in_mock = listed
            .iter()
            .find(|(name, _)| *name == interface)
            .map(|(_, permission)| *permission);
        assert_eq!(in_mock, in_table, "{interface}");
    }
}

#[test]
fn a_call_the_grant_lacks_answers_denied_in_a_unit_test() {
    assert!(matches!(
        secrets::get("token"),
        Err(Error::Denied(permission)) if permission == "secrets"
    ));
    mock::grant(&["secrets"]);
    secrets::set("token", "t").unwrap();
    assert_eq!(secrets::get("token").unwrap().as_deref(), Some("t"));
    assert!(mock::calls().contains(&"hestia:kernel/secrets#get".to_owned()));
}
