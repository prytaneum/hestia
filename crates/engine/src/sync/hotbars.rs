//! `hotbar.nbt`, merged slot by slot; what is not a toolbar is left alone.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use fastnbt::Value;

use super::reconcile;

const TOOLBARS: usize = 9;
const SLOTS: usize = 9;

type Root = HashMap<String, Value>;

pub fn merge(baseline: &Path, store: &Path, data: &Path) -> Result<()> {
    let stored = read(store)?;
    let local = read(data)?;
    if stored.is_none() && local.is_none() {
        return Ok(());
    }
    let mut stored = stored.unwrap_or_default();
    let mut local = local.unwrap_or_default();
    let base = read(baseline).ok().flatten().unwrap_or_default();
    let data_newer = reconcile::newer(data, store);

    for toolbar in 0..TOOLBARS {
        let key = toolbar.to_string();
        for slot in 0..SLOTS {
            let settled = reconcile::one(
                item(&base, &key, slot),
                item(&stored, &key, slot),
                item(&local, &key, slot),
                data_newer,
            )
            .cloned();
            let Some(settled) = settled else {
                continue;
            };
            set(&mut stored, &key, slot, settled.clone());
            set(&mut local, &key, slot, settled);
        }
    }

    let agreed = encode(&stored)?;
    reconcile::write_if_changed(data, &encode(&local)?)?;
    reconcile::write_if_changed(store, &agreed)?;
    reconcile::write_if_changed(baseline, &agreed)
}

fn read(path: &Path) -> Result<Option<Root>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    let root = fastnbt::from_bytes(&bytes)
        .with_context(|| format!("{} is not a readable hotbar file", path.display()))?;
    Ok(Some(root))
}

fn encode(root: &Root) -> Result<Vec<u8>> {
    fastnbt::to_bytes(root).context("cannot encode the hotbars")
}

fn item<'a>(root: &'a Root, toolbar: &str, slot: usize) -> Option<&'a Value> {
    match root.get(toolbar) {
        Some(Value::List(items)) => items.get(slot),
        _ => None,
    }
}

fn set(root: &mut Root, toolbar: &str, slot: usize, value: Value) {
    let entry = root
        .entry(toolbar.to_string())
        .or_insert_with(|| Value::List(vec![empty(); SLOTS]));
    if !matches!(entry, Value::List(_)) {
        *entry = Value::List(vec![empty(); SLOTS]);
    }
    let Value::List(items) = entry else {
        return;
    };
    while items.len() <= slot {
        items.push(empty());
    }
    items[slot] = value;
}

fn empty() -> Value {
    Value::Compound(HashMap::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotbar(slots: &[(usize, usize, &str)]) -> Vec<u8> {
        let mut root = Root::new();
        for (toolbar, slot, id) in slots {
            let mut stack = HashMap::new();
            stack.insert("id".to_string(), Value::String((*id).to_string()));
            set(
                &mut root,
                &toolbar.to_string(),
                *slot,
                Value::Compound(stack),
            );
        }
        encode(&root).unwrap()
    }

    fn ids(path: &Path) -> Vec<String> {
        let root = read(path).unwrap().unwrap_or_default();
        let mut found = Vec::new();
        for toolbar in 0..TOOLBARS {
            for slot in 0..SLOTS {
                if let Some(Value::Compound(stack)) = item(&root, &toolbar.to_string(), slot) {
                    if let Some(Value::String(id)) = stack.get("id") {
                        found.push(id.clone());
                    }
                }
            }
        }
        found.sort();
        found
    }

    #[test]
    fn two_instances_saving_different_toolbars_both_survive() {
        let dir = tempfile::tempdir().unwrap();
        let baseline = dir.path().join("baseline.nbt");
        let store = dir.path().join("store.nbt");
        let data = dir.path().join("data.nbt");
        std::fs::write(&store, hotbar(&[(0, 0, "minecraft:stone")])).unwrap();
        std::fs::write(&data, hotbar(&[(3, 4, "minecraft:torch")])).unwrap();

        merge(&baseline, &store, &data).unwrap();

        assert_eq!(ids(&data), ["minecraft:stone", "minecraft:torch"]);
        assert_eq!(ids(&store), ["minecraft:stone", "minecraft:torch"]);
    }

    #[test]
    fn a_slot_the_player_emptied_settles_as_the_edit_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let baseline = dir.path().join("baseline.nbt");
        let store = dir.path().join("store.nbt");
        let data = dir.path().join("data.nbt");
        let saved = hotbar(&[(0, 0, "minecraft:stone")]);
        std::fs::write(&store, &saved).unwrap();
        std::fs::write(&data, &saved).unwrap();
        merge(&baseline, &store, &data).unwrap();

        let mut cleared = Root::new();
        set(&mut cleared, "0", 0, empty());
        std::fs::write(&data, encode(&cleared).unwrap()).unwrap();
        merge(&baseline, &store, &data).unwrap();

        assert!(ids(&store).is_empty());
    }
}
