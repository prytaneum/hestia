//! Where a program may be, written for the platform that will run it. The rule
//! is syntactic and independent of the OS doing the check, so a manifest reads
//! the same on every machine, and it runs before any file is opened: a UNC or
//! device path would make Windows authenticate to a remote host or wait on a
//! device.

use crate::manifest::{Os, ProgramEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    Unix,
    Windows,
}

impl Family {
    pub(super) const HERE: Family = if cfg!(windows) {
        Family::Windows
    } else {
        Family::Unix
    };

    fn of(os: Os) -> Family {
        match os {
            Os::Windows => Family::Windows,
            Os::Linux | Os::Macos => Family::Unix,
        }
    }

    /// The family an entry is written for: its `os`, or, with none, the one its
    /// path is written in. A path in neither form is judged as this platform's,
    /// which refuses it.
    pub(super) fn of_entry(entry: &ProgramEntry) -> Family {
        match entry.os {
            Some(os) => Family::of(os),
            None if entry.path.starts_with('/') => Family::Unix,
            None if path_has_drive(&entry.path) => Family::Windows,
            None => Family::HERE,
        }
    }
}

fn path_has_drive(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() > 1 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

const DEVICES: [&str; 8] = [
    "con", "prn", "aux", "nul", "conin$", "conout$", "clock$", "$mft",
];

pub(super) fn check(path: &str, family: Family) -> Result<(), &'static str> {
    match family {
        Family::Unix => unix(path),
        Family::Windows => windows(path),
    }
}

fn unix(path: &str) -> Result<(), &'static str> {
    if path.starts_with('/') {
        Ok(())
    } else {
        Err("write an absolute path")
    }
}

fn windows(path: &str) -> Result<(), &'static str> {
    let drive =
        path.len() > 3 && path_has_drive(path) && matches!(path.as_bytes()[2], b'/' | b'\\');
    if !drive {
        return Err("write an absolute path on a drive, like C:\\tools\\tool.exe");
    }
    let rest = &path[3..];
    if rest.contains([':', '?', '*', '<', '>', '|', '"']) || rest.chars().any(char::is_control) {
        return Err("a path may not hold a stream, wildcard or control character");
    }
    if !path.to_ascii_lowercase().ends_with(".exe") {
        return Err("a program must be an .exe file");
    }
    if rest.split(['/', '\\']).any(device) {
        return Err("a path may not name a device");
    }
    Ok(())
}

fn device(component: &str) -> bool {
    let stem = component
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_lowercase();
    let numbered = |prefix: &str| {
        stem.strip_prefix(prefix)
            .is_some_and(|n| n.len() == 1 && matches!(n.as_bytes()[0], b'1'..=b'9'))
    };
    DEVICES.contains(&stem.as_str()) || numbered("com") || numbered("lpt")
}

#[cfg(test)]
mod tests {
    use super::{check, Family};

    #[test]
    fn a_program_path_is_judged_for_the_platform_it_is_written_for() {
        let unix = [
            ("/usr/bin/git", true),
            ("relative/tool", false),
            (r"C:\tools\t.exe", false),
            (r"\\server\share\t.exe", false),
        ];
        let windows = [
            (r"C:\tools\t.exe", true),
            ("c:/tools/T.EXE", true),
            (r"C:\tools\t", false),
            (r"C:\tools\t.bat", false),
            (r"C:\tools\t.cmd", false),
            (r"C:\tools\t.exe ", false),
            (r"C:\tools\t.exe:stream", false),
            (r"C:\tools\t.txt:x.exe", false),
            (r"C:\tools\nul.exe", false),
            (r"C:\tools\COM1\t.exe", false),
            (r"C:\tools\*.exe", false),
            (r"\\server\share\t.exe", false),
            ("//server/share/t.exe", false),
            (r"\\?\C:\tools\t.exe", false),
            (r"\\.\pipe\t.exe", false),
            (r"\\.\C:\t.exe", false),
            (r"\tools\t.exe", false),
            (r"C:t.exe", false),
            ("/usr/bin/git", false),
            ("t.exe", false),
        ];
        for (path, ok) in unix {
            assert_eq!(check(path, Family::Unix).is_ok(), ok, "unix {path}");
        }
        for (path, ok) in windows {
            assert_eq!(check(path, Family::Windows).is_ok(), ok, "windows {path}");
        }
    }
}
