//! Rendering of `View`s: plain text on stdout — dimmed labels on a terminal,
//! bare when piped or redirected (so `| grep` and `> file` keep working).
//! Tables too tall for the terminal hand off to the fullscreen pager when the
//! invocation is interactive.

use std::io::{self, IsTerminal};

use anyhow::Result;
use ratatui::crossterm::style::Stylize;

use super::cell::Cell;
use super::session;
use super::session::pager::PagerScreen;
use super::view::View;

pub fn show(view: View) -> Result<()> {
    if super::json::active() {
        return Ok(super::json::show(view)?);
    }
    match view {
        View::Line(text) => line(&text)?,
        View::Note(text) | View::Empty(text) => note(&text)?,
        View::Warning(text) => warning(&text)?,
        View::Detail(rows) => detail(&rows)?,
        View::Table {
            title,
            headers,
            rows,
            ..
        } => {
            let rows: Vec<Vec<String>> = rows
                .iter()
                .map(|row| row.iter().map(|cell| cell.text().to_string()).collect())
                .collect();
            if !paged(&title, &headers, &rows)? {
                print_table(&headers, &rows)?;
            }
        }
        View::Json(value) => line(&format!("{value:#}"))?,
    }
    Ok(())
}

/// Write one line to stdout. Returns rather than panics on a closed pipe, which
/// is what `println!` does and what made `hestia … | head` a crash report.
fn line(text: &str) -> io::Result<()> {
    use std::io::Write;
    writeln!(io::stdout().lock(), "{text}")
}

/// Page a table too tall for the terminal in a fullscreen session; `false`
/// hands it back for plain printing.
fn paged(title: &str, headers: &[String], rows: &[Vec<String>]) -> Result<bool> {
    if rows.is_empty() || !stdout_is_tty() || !super::is_interactive() {
        return Ok(false);
    }
    let height = ratatui::crossterm::terminal::size()
        .map(|(_, h)| h)
        .unwrap_or(0);
    if rows.len() as u16 + 3 <= height {
        return Ok(false);
    }
    session::run(
        PagerScreen::new(title, headers.to_vec(), rows.to_vec()),
        None,
    )?;
    Ok(true)
}

fn stdout_is_tty() -> bool {
    io::stdout().is_terminal()
}

fn note(text: &str) -> io::Result<()> {
    match stdout_is_tty() {
        true => line(&text.dark_grey().to_string()),
        false => line(text),
    }
}

/// A warning keeps the `warning:` prefix when redirected, so a script that
/// captures stdout can still see the operation was degraded.
fn warning(text: &str) -> io::Result<()> {
    match stdout_is_tty() {
        true => line(&format!("{} {text}", "warning:".yellow())),
        false => line(&format!("warning: {text}")),
    }
}

fn detail(rows: &[(String, Cell)]) -> io::Result<()> {
    let rows: Vec<_> = rows.iter().filter(|(_, cell)| cell.is_shown()).collect();
    let width = rows.iter().map(|(k, _)| k.len()).max().unwrap_or(0);
    for (key, cell) in rows {
        let value = cell.text();
        let label = format!("{key:width$}");
        match stdout_is_tty() {
            true => line(&format!("{}  {value}", label.dark_grey()))?,
            false => line(&format!("{label}  {value}"))?,
        }
    }
    Ok(())
}

/// Print a left-aligned table (the plain path).
fn print_table(headers: &[String], rows: &[Vec<String>]) -> io::Result<()> {
    let header_refs: Vec<&str> = headers.iter().map(String::as_str).collect();
    let widths = column_widths(&header_refs, rows);
    let header = render_row(&header_refs, &widths);
    match stdout_is_tty() {
        true => line(&header.dark_grey().to_string())?,
        false => line(&header)?,
    }
    for row in rows {
        line(&render_row(row, &widths))?;
    }
    Ok(())
}

/// The width each column must be padded to: the widest of its header and cells.
pub fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                widths[i] = widths[i].max(cell.len());
            }
        }
    }
    widths
}

/// Render one row's cells left-aligned to `widths`, joined by two spaces.
pub fn render_row<S: AsRef<str>>(cells: &[S], widths: &[usize]) -> String {
    let line: Vec<String> = cells
        .iter()
        .enumerate()
        .map(|(i, c)| {
            format!(
                "{:width$}",
                c.as_ref(),
                width = widths.get(i).copied().unwrap_or(0)
            )
        })
        .collect();
    line.join("  ").trim_end().to_string()
}

/// Render a byte count in human units.
pub fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
