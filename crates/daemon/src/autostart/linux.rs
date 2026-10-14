use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

const ENTRY_NAME: &str = "hestiad.desktop";

fn autostart_dir() -> Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Ok(PathBuf::from(xdg).join("autostart"));
        }
    }
    let home = std::env::var_os("HOME").context("cannot resolve autostart dir: HOME is unset")?;
    Ok(PathBuf::from(home).join(".config").join("autostart"))
}

/// The command line that brings this daemon back at login. Under an AppImage the
/// executable sits on a mount that is gone by then, so the image itself starts
/// it, through the shell.
fn exec_line() -> Result<String> {
    if let Some(image) = common::appimage::image() {
        return Ok(format!(
            "{} {}",
            exec_arg(&image)?,
            common::app::DESKTOP_DAEMON_ARG
        ));
    }
    let exe = std::env::current_exe().context("cannot resolve daemon executable path")?;
    Ok(format!("{} serve", exec_arg(&exe)?))
}

/// `path` as one quoted `Exec` argument. The Desktop Entry spec escapes `"`,
/// `` ` ``, `$` and `\` inside quotes, then applies the string escape to that
/// result, so each of those costs two backslashes and `\` itself four; `%` opens
/// a field code unless doubled.
fn exec_arg(path: &Path) -> Result<String> {
    let path = path
        .to_str()
        .with_context(|| format!("{} is not valid UTF-8", path.display()))?;
    let mut arg = String::from("\"");
    for c in path.chars() {
        match c {
            '"' | '`' | '$' => {
                arg.push_str("\\\\");
                arg.push(c);
            }
            '\\' => arg.push_str("\\\\\\\\"),
            '%' => arg.push_str("%%"),
            _ => arg.push(c),
        }
    }
    arg.push('"');
    Ok(arg)
}

fn entry_contents() -> Result<String> {
    Ok(include_str!("../../assets/autostart.desktop")
        .replace("@NAME@", common::app::NAME)
        .replace("@EXEC@", &exec_line()?))
}

pub(super) fn enable() -> Result<()> {
    let dir = autostart_dir()?;
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(ENTRY_NAME), entry_contents()?).context("cannot enable autostart")
}

pub(super) fn disable() -> Result<()> {
    let dir = autostart_dir()?;
    let _ = std::fs::remove_file(dir.join(ENTRY_NAME));
    Ok(())
}

pub(super) fn is_enabled() -> bool {
    autostart_dir()
        .map(|dir| dir.join(ENTRY_NAME).exists())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_exec_argument_survives_both_escape_rules() {
        assert_eq!(
            exec_arg(Path::new("/home/u/My Apps/Hestia.AppImage")).unwrap(),
            r#""/home/u/My Apps/Hestia.AppImage""#
        );
        assert_eq!(
            exec_arg(Path::new(r#"/a"b$c`d\e%f"#)).unwrap(),
            r#""/a\\"b\\$c\\`d\\\\e%%f""#
        );
    }
}
