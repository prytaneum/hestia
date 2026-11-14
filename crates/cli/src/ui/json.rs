//! The `--json` rendering of `View`s. Under `--json` stdout carries exactly one
//! document per command — `{"data": …, "warnings": […]}` — written when the
//! command finishes; everything a command shows is collected until then. A
//! stream (`logs -f`) opts into one object per line instead. Failures are one
//! object on stderr. The shape is documented in `docs/cli.md`.

use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use client::proto::warning::WarningInfo;
use client::IpcError;
use serde_json::{json, Map, Value};

use super::cell::Cell;
use super::view::{key_of, View};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static STREAMING: AtomicBool = AtomicBool::new(false);
static COLLECTED: Mutex<Collected> = Mutex::new(Collected {
    data: Vec::new(),
    warnings: Vec::new(),
});

struct Collected {
    data: Vec<Value>,
    warnings: Vec<Value>,
}

pub fn enable() {
    ACTIVE.store(true, Ordering::Relaxed);
}

pub fn active() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Switch to one object per line, flushed as each view is shown, for a command
/// that never settles into a single document. What was collected so far is
/// written first, one object per line.
pub fn stream() -> io::Result<()> {
    if !active() {
        return Ok(());
    }
    STREAMING.store(true, Ordering::Relaxed);
    let mut collected = collected();
    let pending: Vec<Value> = std::mem::take(&mut collected.data)
        .into_iter()
        .chain(std::mem::take(&mut collected.warnings))
        .collect();
    drop(collected);
    pending.iter().try_for_each(write_line)
}

fn collected() -> MutexGuard<'static, Collected> {
    COLLECTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(super) fn show(view: View) -> io::Result<()> {
    if let View::Warning(text) = &view {
        return warn(None, text, None);
    }
    emit(view_value(&view), |collected, value| {
        collected.data.push(value)
    })
}

pub(super) fn warn_info(info: &WarningInfo) -> io::Result<()> {
    let kind = serde_json::to_value(info)
        .ok()
        .and_then(|value| value.get("kind").and_then(Value::as_str).map(str::to_owned));
    warn(kind, &info.to_string(), Some(info.hint()))
}

fn warn(kind: Option<String>, message: &str, hint: Option<String>) -> io::Result<()> {
    emit(warning_value(kind, message, hint), |collected, value| {
        collected.warnings.push(value)
    })
}

fn emit(value: Value, collect: impl FnOnce(&mut Collected, Value)) -> io::Result<()> {
    if STREAMING.load(Ordering::Relaxed) {
        return write_line(&value);
    }
    collect(&mut collected(), value);
    Ok(())
}

/// Write the collected document; a no-op unless `--json` is on and the command
/// did not stream.
pub fn finish() -> io::Result<()> {
    if !active() || STREAMING.load(Ordering::Relaxed) {
        return Ok(());
    }
    let mut collected = collected();
    let data = std::mem::take(&mut collected.data);
    let warnings = std::mem::take(&mut collected.warnings);
    drop(collected);
    write_line(&document(data, warnings))
}

pub fn print_error(error: &anyhow::Error) {
    eprintln!("{}", error_value(error));
}

fn write_line(value: &Value) -> io::Result<()> {
    let mut out = io::stdout().lock();
    writeln!(out, "{value}")?;
    out.flush()
}

fn document(mut data: Vec<Value>, warnings: Vec<Value>) -> Value {
    let data = match data.len() {
        0 => Value::Null,
        1 => data.remove(0),
        _ => Value::Array(data),
    };
    json!({ "data": data, "warnings": warnings })
}

fn warning_value(kind: Option<String>, message: &str, hint: Option<String>) -> Value {
    json!({ "kind": kind, "message": message, "hint": hint })
}

fn view_value(view: &View) -> Value {
    match view {
        View::Line(text) | View::Note(text) | View::Warning(text) => json!({ "message": text }),
        View::Detail(rows) => Value::Object(detail_object(rows)),
        View::Table { keys, rows, .. } => Value::Array(
            rows.iter()
                .map(|row| {
                    keys.iter()
                        .zip(row)
                        .map(|(key, cell)| (key.clone(), cell.value().clone()))
                        .collect::<Map<_, _>>()
                        .into()
                })
                .collect(),
        ),
        View::Empty(_) => json!([]),
        View::Json(value) => value.clone(),
    }
}

fn detail_object(rows: &[(String, Cell)]) -> Map<String, Value> {
    rows.iter()
        .map(|(key, cell)| (key_of(key), cell.value().clone()))
        .collect()
}

/// A daemon failure keeps the `kind` and `code` of its `ErrorInfo`; anything
/// the CLI itself refused is `local`.
fn error_value(error: &anyhow::Error) -> Value {
    let daemon = error.chain().find_map(|cause| match cause.downcast_ref() {
        Some(IpcError::Daemon { code, info, .. }) => Some((code, info)),
        _ => None,
    });
    let (kind, code) = match daemon {
        Some((code, info)) => (
            info.get("kind").and_then(Value::as_str).unwrap_or(code),
            code.as_str(),
        ),
        None => ("local", "local"),
    };
    json!({ "error": { "kind": kind, "code": code, "message": format!("{error:#}") } })
}

#[cfg(test)]
mod tests {
    use anyhow::Context;
    use client::proto::process::ProcessState;

    use super::*;

    fn rendered(views: Vec<View>) -> Value {
        document(views.iter().map(view_value).collect(), Vec::new())
    }

    #[test]
    fn a_table_is_an_array_keyed_by_snake_case_columns_of_typed_values() {
        let table = View::table(
            "Servers",
            [
                "NAME",
                "GAME VERSION",
                "PID",
                "SIZE",
                "UPTIME",
                "AGE",
                "ON",
                "STATE",
                "TAKES",
                "LOADER",
            ],
            vec![vec![
                Cell::from("smp"),
                "1.21".into(),
                Cell::count(112751u32),
                Cell::bytes(1_200_000_000),
                Cell::seconds(90, "1m"),
                Cell::time(1_700_000_000, "3d ago"),
                Cell::flag(true, "yes"),
                Cell::token("stopped (1)", &ProcessState::Exited),
                Cell::list("mods, datapacks", ["mod", "data_pack"]),
                Cell::absent("-"),
            ]],
        )
        .keyed([
            "name", "version", "pid", "size", "uptime", "created", "on", "state", "takes", "loader",
        ]);
        assert_eq!(
            rendered(vec![table]),
            json!({
                "data": [{
                    "name": "smp",
                    "version": "1.21",
                    "pid": 112751,
                    "size": 1_200_000_000u64,
                    "uptime": 90,
                    "created": "2023-11-14T22:13:20Z",
                    "on": true,
                    "state": "exited",
                    "takes": ["mod", "data_pack"],
                    "loader": null
                }],
                "warnings": []
            })
        );
        let size = Cell::bytes(1_200_000_000);
        assert_eq!((size.text(), Cell::from("a").text()), ("1.2 GB", "a"));
    }

    #[test]
    fn a_detail_is_one_object_and_a_line_carries_a_message() {
        let detail = View::detail([("max memory", "4G")]);
        assert_eq!(
            rendered(vec![detail])["data"],
            json!({ "max_memory": "4G" })
        );
        assert_eq!(
            rendered(vec![View::line("done")])["data"],
            json!({ "message": "done" })
        );
        assert_eq!(
            rendered(vec![View::empty("no servers yet")])["data"],
            json!([])
        );
    }

    #[test]
    fn several_views_become_one_array_and_warnings_ride_beside_them() {
        let warnings = vec![warning_value(
            Some("sync_unit_skipped".into()),
            "skipped",
            Some("launch again".into()),
        )];
        let data = vec![
            view_value(&View::line("created")),
            view_value(&View::detail([("id", "a1")])),
        ];
        assert_eq!(
            document(data, warnings),
            json!({
                "data": [{ "message": "created" }, { "id": "a1" }],
                "warnings": [{ "kind": "sync_unit_skipped", "message": "skipped", "hint": "launch again" }]
            })
        );
    }

    #[test]
    fn a_daemon_failure_keeps_its_kind_and_a_local_one_is_local() {
        let daemon = anyhow::Error::new(IpcError::Daemon {
            code: "not_found".into(),
            message: "no server 'smp'".into(),
            info: json!({ "kind": "entry_not_found" }),
        });
        assert_eq!(
            error_value(&daemon),
            json!({ "error": { "kind": "entry_not_found", "code": "not_found", "message": "no server 'smp'" } })
        );
        let local = anyhow::anyhow!("no interactive terminal").context("cannot choose");
        assert_eq!(
            error_value(&local),
            json!({ "error": { "kind": "local", "code": "local", "message": "cannot choose: no interactive terminal" } })
        );
        let wrapped = Err::<(), _>(IpcError::ConnectionLost)
            .context("the daemon is not running")
            .unwrap_err();
        assert_eq!(error_value(&wrapped)["error"]["kind"], "local");
    }
}
