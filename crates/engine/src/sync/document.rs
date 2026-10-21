//! `options.txt` as the game wrote it: re-rendering would drop its comments,
//! reorder its keys and restamp the file on every pass.

use std::path::Path;

use anyhow::{bail, Result};

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_LINES: usize = 16_384;

#[derive(Clone)]
pub struct Document {
    lines: Vec<Line>,
    ending: String,
}

#[derive(Clone)]
struct Line {
    text: String,
    ending: String,
    pair: Option<(String, String)>,
}

impl Default for Document {
    fn default() -> Self {
        Document {
            lines: Vec::new(),
            ending: "\n".to_string(),
        }
    }
}

impl Document {
    pub fn read(path: &Path) -> Result<Option<Document>> {
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        if bytes.len() > MAX_BYTES {
            bail!("options.txt is larger than {MAX_BYTES} bytes");
        }
        if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
            bail!("options.txt is UTF-16, which the game does not write");
        }
        let text =
            String::from_utf8(bytes).map_err(|_| anyhow::anyhow!("options.txt is not UTF-8"))?;
        Ok(Some(Document::parse(&text)?))
    }

    pub fn parse(text: &str) -> Result<Document> {
        let mut lines = Vec::new();
        let mut rest = text;
        while !rest.is_empty() {
            let (line, ending, remainder) = split_line(rest);
            if lines.len() == MAX_LINES {
                bail!("options.txt has more than {MAX_LINES} lines");
            }
            lines.push(Line {
                text: line.to_string(),
                ending: ending.to_string(),
                pair: pair(line),
            });
            rest = remainder;
        }
        let ending = lines
            .iter()
            .map(|line| line.ending.as_str())
            .find(|ending| !ending.is_empty())
            .unwrap_or("\n")
            .to_string();
        Ok(Document { lines, ending })
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.lines.iter().find_map(|line| match &line.pair {
            Some((name, value)) if name == key => Some(value.as_str()),
            _ => None,
        })
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.lines
            .iter()
            .filter_map(|line| line.pair.as_ref().map(|(key, _)| key.as_str()))
    }

    pub fn set(&mut self, key: &str, value: &str) {
        if let Some(line) = self
            .lines
            .iter_mut()
            .find(|line| matches!(&line.pair, Some((name, _)) if name == key))
        {
            if line.pair.as_ref().is_some_and(|(_, old)| old == value) {
                return;
            }
            line.text = format!("{key}:{value}");
            line.pair = Some((key.to_string(), value.to_string()));
            return;
        }
        if let Some(last) = self.lines.last_mut() {
            if last.ending.is_empty() {
                last.ending = self.ending.clone();
            }
        }
        self.lines.push(Line {
            text: format!("{key}:{value}"),
            ending: self.ending.clone(),
            pair: Some((key.to_string(), value.to_string())),
        });
    }

    pub fn render(&self) -> String {
        let mut text = String::new();
        for line in &self.lines {
            text.push_str(&line.text);
            text.push_str(&line.ending);
        }
        text
    }
}

fn split_line(text: &str) -> (&str, &str, &str) {
    match text.find('\n') {
        Some(index) => {
            let (line, rest) = text.split_at(index);
            match line.strip_suffix('\r') {
                Some(line) => (line, "\r\n", &rest[1..]),
                None => (line, "\n", &rest[1..]),
            }
        }
        None => (text, "", ""),
    }
}

fn pair(line: &str) -> Option<(String, String)> {
    if line.starts_with('#') {
        return None;
    }
    let (key, value) = line.split_once(':')?;
    if key.is_empty() || key.trim() != key {
        return None;
    }
    Some((key.to_string(), value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_round_trips_byte_for_byte() {
        let text = "# mine\r\nguiScale:3\r\n\r\nfov:70\r\nnot a pair\r\n";
        let document = Document::parse(text).unwrap();
        assert_eq!(document.render(), text);
    }

    #[test]
    fn setting_a_value_leaves_everything_else_where_it_was() {
        let document = &mut Document::parse("# mine\nguiScale:3\n\nfov:70\n").unwrap();
        document.set("guiScale", "4");
        assert_eq!(document.render(), "# mine\nguiScale:4\n\nfov:70\n");
    }

    #[test]
    fn a_new_key_is_appended_in_the_files_own_ending() {
        let document = &mut Document::parse("guiScale:3\r\n").unwrap();
        document.set("fov", "90");
        assert_eq!(document.render(), "guiScale:3\r\nfov:90\r\n");
    }

    #[test]
    fn a_file_with_no_trailing_newline_gains_one_only_when_appended_to() {
        let document = &mut Document::parse("guiScale:3").unwrap();
        assert_eq!(document.render(), "guiScale:3");
        document.set("fov", "90");
        assert_eq!(document.render(), "guiScale:3\nfov:90\n");
    }

    #[test]
    fn a_value_may_hold_the_separator() {
        let document = Document::parse("lastServer:example.net:25565\n").unwrap();
        assert_eq!(document.get("lastServer"), Some("example.net:25565"));
    }

    #[test]
    fn a_comment_is_not_a_pair() {
        let document = Document::parse("#guiScale:3\nfov:70\n").unwrap();
        assert_eq!(document.get("guiScale"), None);
        assert_eq!(document.keys().collect::<Vec<_>>(), ["fov"]);
    }

    #[test]
    fn a_file_that_is_not_utf8_is_an_error_rather_than_an_empty_document() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("options.txt");
        std::fs::write(&path, [0xFF, 0xFE, 0x67, 0x00]).unwrap();
        assert!(Document::read(&path).is_err());
    }
}
