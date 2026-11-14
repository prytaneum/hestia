//! `servers.dat`, merged row by row. A row carries no id, so identity comes
//! from the agreement: a row whose address still matches is the same row.

use std::path::Path;

use anyhow::Result;
use fastnbt::Value;

use super::reconcile;
use crate::minecraft::servers::{self, Row};

pub fn merge(baseline_dir: &Path, store_dir: &Path, data_dir: &Path) -> Result<()> {
    let stored = servers::read_rows(store_dir)?;
    let local = servers::read_rows(data_dir)?;
    if stored.is_empty() && local.is_empty() {
        return Ok(());
    }
    let base = servers::read_rows(baseline_dir).unwrap_or_default();
    let data_newer = reconcile::newer(&servers::path(data_dir), &servers::path(store_dir));

    let local_of = invert(&paired(&local, &base), base.len());
    let store_of = invert(&paired(&stored, &base), base.len());
    let settled: Vec<Option<Row>> = (0..base.len())
        .map(|agreed| {
            settle(
                &base[agreed],
                store_of[agreed].map(|row| &stored[row]),
                local_of[agreed].map(|row| &local[row]),
                data_newer,
            )
        })
        .collect();

    let for_data = dedupe(
        rebuild(&local, &paired(&local, &base), &settled, true)
            .into_iter()
            .chain(added(&stored, &paired(&stored, &base))),
    );
    let for_store = dedupe(
        rebuild(&stored, &paired(&stored, &base), &settled, false)
            .into_iter()
            .chain(added(&local, &paired(&local, &base))),
    );
    let agreed: Vec<Row> = for_data
        .iter()
        .filter(|row| !hidden(row))
        .cloned()
        .collect();

    if for_data != local {
        servers::write_rows(data_dir, &for_data)?;
    }
    if for_store != stored {
        servers::write_rows(store_dir, &for_store)?;
    }
    if agreed != base {
        servers::write_rows(baseline_dir, &agreed)?;
    }
    Ok(())
}

fn settle(base: &Row, stored: Option<&Row>, local: Option<&Row>, data_newer: bool) -> Option<Row> {
    match (stored, local) {
        (Some(s), Some(l)) if s == l => Some(s.clone()),
        (Some(s), Some(l)) => match (s != base, l != base) {
            (true, false) => Some(s.clone()),
            (false, true) => Some(l.clone()),
            _ => Some(if data_newer { l.clone() } else { s.clone() }),
        },
        (Some(s), None) => (s != base).then(|| s.clone()),
        (None, Some(l)) => (l != base).then(|| l.clone()),
        (None, None) => None,
    }
}

fn paired(rows: &[Row], base: &[Row]) -> Vec<Option<usize>> {
    let same: [fn(&Row, &Row) -> bool; 3] = [
        |row, candidate| row == candidate,
        |row, candidate| {
            let address = text(row, "ip");
            !address.is_empty() && address == text(candidate, "ip")
        },
        |row, candidate| {
            let name = text(row, "name");
            !name.is_empty() && name == text(candidate, "name")
        },
    ];
    let mut pairs = vec![None; rows.len()];
    let mut taken = vec![false; base.len()];
    for matches in same {
        for (index, row) in rows.iter().enumerate() {
            if pairs[index].is_some() || hidden(row) {
                continue;
            }
            let found = base
                .iter()
                .enumerate()
                .filter(|(agreed, candidate)| !taken[*agreed] && matches(row, candidate))
                .min_by_key(|(agreed, _)| agreed.abs_diff(index))
                .map(|(agreed, _)| agreed);
            if let Some(agreed) = found {
                pairs[index] = Some(agreed);
                taken[agreed] = true;
            }
        }
    }
    pairs
}

fn invert(pairs: &[Option<usize>], len: usize) -> Vec<Option<usize>> {
    let mut inverted = vec![None; len];
    for (row, agreed) in pairs.iter().enumerate() {
        if let Some(agreed) = agreed {
            inverted[*agreed] = Some(row);
        }
    }
    inverted
}

fn rebuild(
    rows: &[Row],
    pairs: &[Option<usize>],
    settled: &[Option<Row>],
    keep_hidden: bool,
) -> Vec<Row> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, row)| match pairs[index] {
            Some(agreed) => settled[agreed].clone(),
            None if hidden(row) => keep_hidden.then(|| row.clone()),
            None => Some(row.clone()),
        })
        .collect()
}

fn added<'a>(rows: &'a [Row], pairs: &'a [Option<usize>]) -> impl Iterator<Item = Row> + 'a {
    rows.iter()
        .enumerate()
        .filter(|(index, row)| pairs[*index].is_none() && !hidden(row))
        .map(|(_, row)| row.clone())
}

fn dedupe(rows: impl Iterator<Item = Row>) -> Vec<Row> {
    let mut placed: Vec<String> = Vec::new();
    let mut kept = Vec::new();
    for row in rows {
        let identity = match text(&row, "ip").is_empty() {
            true => text(&row, "name"),
            false => text(&row, "ip"),
        };
        if !hidden(&row) && !identity.is_empty() {
            if placed.contains(&identity) {
                continue;
            }
            placed.push(identity);
        }
        kept.push(row);
    }
    kept
}

fn hidden(row: &Row) -> bool {
    matches!(row.get("hidden"), Some(Value::Byte(byte)) if *byte != 0)
}

fn text(row: &Row, key: &str) -> String {
    match row.get(key) {
        Some(Value::String(value)) => value.trim().to_lowercase(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(name: &str, address: &str) -> Row {
        let mut row = Row::new();
        row.insert("name".to_string(), Value::String(name.to_string()));
        row.insert("ip".to_string(), Value::String(address.to_string()));
        row
    }

    fn hidden_row(address: &str) -> Row {
        let mut row = row("", address);
        row.insert("hidden".to_string(), Value::Byte(1));
        row
    }

    fn names(rows: &[Row]) -> Vec<String> {
        rows.iter().map(|row| text(row, "name")).collect()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        baseline: std::path::PathBuf,
        store: std::path::PathBuf,
        data: std::path::PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let each = |name: &str| {
            let path = dir.path().join(name);
            std::fs::create_dir_all(&path).unwrap();
            path
        };
        Fixture {
            baseline: each("baseline"),
            store: each("store"),
            data: each("data"),
            _dir: dir,
        }
    }

    #[test]
    fn renaming_a_row_is_an_edit_rather_than_a_delete_and_an_add() {
        let f = fixture();
        servers::write_rows(&f.store, &[row("SMP", "smp.example.net")]).unwrap();
        merge(&f.baseline, &f.store, &f.data).unwrap();

        servers::write_rows(&f.data, &[row("Hermitcraft", "smp.example.net")]).unwrap();
        merge(&f.baseline, &f.store, &f.data).unwrap();

        assert_eq!(
            names(&servers::read_rows(&f.store).unwrap()),
            ["hermitcraft"]
        );
    }

    #[test]
    fn a_tag_this_build_does_not_model_survives_the_round_trip() {
        let f = fixture();
        let mut carried = row("SMP", "smp.example.net");
        carried.insert("previewsChat".to_string(), Value::Byte(1));
        servers::write_rows(&f.data, &[carried]).unwrap();

        merge(&f.baseline, &f.store, &f.data).unwrap();

        let stored = servers::read_rows(&f.store).unwrap();
        assert_eq!(stored[0].get("previewsChat"), Some(&Value::Byte(1)));
    }

    #[test]
    fn each_side_keeps_its_own_order_and_new_rows_arrive_at_the_end() {
        let f = fixture();
        servers::write_rows(&f.store, &[row("A", "a.net"), row("B", "b.net")]).unwrap();
        servers::write_rows(&f.data, &[row("B", "b.net"), row("A", "a.net")]).unwrap();
        merge(&f.baseline, &f.store, &f.data).unwrap();

        servers::write_rows(
            &f.store,
            &[row("A", "a.net"), row("B", "b.net"), row("C", "c.net")],
        )
        .unwrap();
        merge(&f.baseline, &f.store, &f.data).unwrap();

        assert_eq!(
            names(&servers::read_rows(&f.data).unwrap()),
            ["b", "a", "c"]
        );
    }

    #[test]
    fn direct_connect_scratch_stays_with_the_instance() {
        let f = fixture();
        servers::write_rows(
            &f.data,
            &[row("SMP", "smp.example.net"), hidden_row("1.2.3.4")],
        )
        .unwrap();

        merge(&f.baseline, &f.store, &f.data).unwrap();

        assert_eq!(names(&servers::read_rows(&f.store).unwrap()), ["smp"]);
        assert_eq!(servers::read_rows(&f.data).unwrap().len(), 2);
    }
}
