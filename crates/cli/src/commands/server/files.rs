//! `hestia server <name> files …` — the server's game directory as a file
//! tree. Paths are relative to that directory and `/`-separated; `upload`
//! takes paths on this machine and copies them in.

use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use clap::Subcommand;
use client::Client;

use crate::ui::{self, View};

#[cfg(windows)]
const FALLBACK_EDITOR: &str = "notepad";
#[cfg(not(windows))]
const FALLBACK_EDITOR: &str = "vi";

#[derive(Subcommand)]
pub enum FilesCmd {
    /// List a directory (the game directory itself when omitted)
    #[command(visible_alias = "ls")]
    List { path: Option<String> },
    /// Print a text file
    Cat { path: String },
    /// Open a text file in $VISUAL/$EDITOR and save it back
    Edit { path: String },
    /// Create or replace a file from standard input
    Write { path: String },
    /// Create a directory
    Mkdir { path: String },
    /// Rename or move a file or directory
    #[command(visible_alias = "mv")]
    Move { from: String, to: String },
    /// Copy a file or directory
    #[command(visible_alias = "cp")]
    Copy { from: String, to: String },
    /// Delete files or directories
    #[command(visible_alias = "rm")]
    Remove {
        #[arg(required = true)]
        paths: Vec<String>,
    },
    /// Copy files or directories from this machine into the server
    Upload {
        #[arg(required = true)]
        sources: Vec<PathBuf>,
        #[arg(
            long,
            default_value = "",
            help = "Directory in the server to copy into"
        )]
        to: String,
    },
    /// Write files or directories into a new .zip
    Zip {
        #[arg(required = true)]
        paths: Vec<String>,
        #[arg(short, long, help = "The .zip to write")]
        output: String,
    },
    /// Unpack a .zip or .tar.gz
    Unzip {
        archive: String,
        #[arg(
            long,
            help = "Directory to unpack into (beside the archive when omitted)"
        )]
        to: Option<String>,
    },
}

pub(super) async fn run(client: &Client, server: &str, cmd: FilesCmd) -> Result<()> {
    let files = client.files();
    match cmd {
        FilesCmd::List { path } => {
            let path = path.unwrap_or_default();
            let entries = files.list(server, &path).await?;
            if entries.is_empty() {
                return ui::show(View::note("empty directory"));
            }
            ui::show(View::table(
                if path.is_empty() { "/" } else { path.as_str() },
                ["NAME", "SIZE"],
                entries
                    .into_iter()
                    .map(|entry| match entry.dir {
                        true => vec![format!("{}/", entry.name), String::new()],
                        false => vec![entry.name, ui::human_bytes(entry.size)],
                    })
                    .collect(),
            ))
        }
        FilesCmd::Cat { path } => {
            let content = files.read(server, &path).await?;
            ui::show(View::line(content.trim_end_matches('\n')))
        }
        FilesCmd::Edit { path } => {
            let content = files.read(server, &path).await?;
            match edit(&path, &content)? {
                Some(edited) => {
                    files.write(server, &path, &edited).await?;
                    ui::show(View::line(format!("saved '{path}'")))
                }
                None => ui::show(View::note("no changes")),
            }
        }
        FilesCmd::Write { path } => {
            let mut content = String::new();
            std::io::stdin()
                .read_to_string(&mut content)
                .context("standard input is not text")?;
            files.write(server, &path, &content).await?;
            ui::show(View::line(format!("wrote '{path}'")))
        }
        FilesCmd::Mkdir { path } => {
            files.mkdir(server, &path).await?;
            ui::show(View::line(format!("created '{path}'")))
        }
        FilesCmd::Move { from, to } => {
            files.rename(server, &from, &to).await?;
            ui::show(View::line(format!("moved '{from}' to '{to}'")))
        }
        FilesCmd::Copy { from, to } => {
            files.copy(server, &from, &to).await?;
            ui::show(View::line(format!("copied '{from}' to '{to}'")))
        }
        FilesCmd::Remove { paths } => {
            files.remove(server, &paths).await?;
            ui::show(View::line(format!("removed {}", paths.join(", "))))
        }
        FilesCmd::Upload { sources, to } => {
            let sources = sources
                .iter()
                .map(|source| absolute(source))
                .collect::<Result<Vec<_>>>()?;
            files.import(server, &to, &sources).await?;
            ui::show(View::line(format!("uploaded {} item(s)", sources.len())))
        }
        FilesCmd::Zip { paths, output } => {
            files.compress(server, &paths, &output).await?;
            ui::show(View::line(format!("wrote '{output}'")))
        }
        FilesCmd::Unzip { archive, to } => {
            let to = to.unwrap_or_else(|| parent_of(&archive));
            files.extract(server, &archive, &to).await?;
            ui::show(View::line(format!("unpacked '{archive}'")))
        }
    }
}

/// The daemon is a separate process and does not share this working directory.
fn absolute(source: &Path) -> Result<String> {
    let path = std::path::absolute(source)
        .with_context(|| format!("cannot resolve {}", source.display()))?;
    path.to_str()
        .map(str::to_string)
        .with_context(|| format!("{} is not valid UTF-8", source.display()))
}

fn parent_of(path: &str) -> String {
    path.rsplit_once('/')
        .map(|(parent, _)| parent.to_string())
        .unwrap_or_default()
}

/// Round-trip `content` through the user's editor; `None` when it came back
/// unchanged.
fn edit(path: &str, content: &str) -> Result<Option<String>> {
    if !std::io::stdin().is_terminal() {
        bail!("editing needs a terminal — pipe into `files write` instead");
    }
    let name = path.rsplit('/').next().unwrap_or(path);
    // The file's own name is kept so the editor picks its syntax from it.
    let dir = tempfile::Builder::new()
        .prefix("hestia-")
        .tempdir()
        .context("cannot create a private scratch directory")?;
    let scratch = dir.path().join(name);
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&scratch)
        .and_then(|mut file| std::io::Write::write_all(&mut file, content.as_bytes()))
        .with_context(|| format!("cannot write {}", scratch.display()))?;

    let command = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| FALLBACK_EDITOR.to_string());
    let mut words = command.split_whitespace();
    let program = words.next().unwrap_or(FALLBACK_EDITOR);
    let outcome = std::process::Command::new(program)
        .args(words)
        .arg(&scratch)
        .status()
        .with_context(|| format!("cannot run the editor '{program}'"))
        .and_then(|status| match status.success() {
            true => std::fs::read_to_string(&scratch)
                .with_context(|| format!("cannot read {}", scratch.display())),
            false => bail!("the editor exited with {status}; nothing was saved"),
        });

    let edited = outcome?;
    Ok((edited != content).then_some(edited))
}
