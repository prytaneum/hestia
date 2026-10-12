//! What may be typed next, read out of the server's own `help` usage. An RCON
//! reply joins its lines with nothing between them, so usages split on `/`.

use std::collections::BTreeSet;

pub struct Typed<'a> {
    pub slash: bool,
    pub words: Vec<&'a str>,
    pub partial: &'a str,
}

impl<'a> Typed<'a> {
    /// `None` for a line carrying control characters, which must not reach a command.
    pub fn parse(line: &'a str) -> Option<Self> {
        let line = line.trim_start();
        if line.chars().any(char::is_control) {
            return None;
        }
        let slash = line.starts_with('/');
        let line = line.strip_prefix('/').unwrap_or(line);
        let mut words: Vec<&str> = line.split_whitespace().collect();
        let partial = match line.ends_with(char::is_whitespace) {
            true => "",
            false => words.pop().unwrap_or(""),
        };
        Some(Typed {
            slash,
            words,
            partial,
        })
    }
}

#[derive(Default, Debug, PartialEq)]
pub struct Candidates {
    pub literals: BTreeSet<String>,
    pub arguments: BTreeSet<String>,
}

pub fn candidates(usage: &str, words: &str) -> Candidates {
    let mut found = Candidates::default();
    for next in following(usage, words).filter_map(|rest| rest.split_whitespace().next()) {
        let inner = next
            .strip_prefix('[')
            .and_then(|inner| inner.strip_suffix(']'))
            .or_else(|| {
                next.strip_prefix('(')
                    .and_then(|inner| inner.strip_suffix(')'))
            })
            .unwrap_or(next);
        for choice in inner.split('|') {
            if choice.starts_with('<') && choice.ends_with('>') {
                found.arguments.insert(choice.to_string());
            } else if is_literal(choice) {
                found.literals.insert(choice.to_string());
            }
        }
    }
    found
}

pub fn alias<'a>(root: &'a str, word: &str) -> Option<&'a str> {
    following(root, "").find_map(|rest| {
        let mut parts = rest.split_whitespace();
        (parts.next() == Some(word) && parts.next() == Some("->"))
            .then(|| parts.next())
            .flatten()
    })
}

fn following<'a>(usage: &'a str, words: &str) -> impl Iterator<Item = &'a str> {
    let marker = match words.is_empty() {
        true => "/".to_string(),
        false => format!("/{words} "),
    };
    usage
        .split(marker.as_str())
        .skip(1)
        .filter_map(|rest| rest.split('/').next())
        .collect::<Vec<_>>()
        .into_iter()
}

fn is_literal(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-:.+".contains(c))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(words: &[&str]) -> BTreeSet<String> {
        words.iter().map(|word| word.to_string()).collect()
    }

    #[test]
    fn a_line_splits_at_the_word_being_typed() {
        let typed = Typed::parse("/time se").unwrap();
        assert!(typed.slash);
        assert_eq!((typed.words, typed.partial), (vec!["time"], "se"));

        let typed = Typed::parse("time set ").unwrap();
        assert_eq!((typed.words, typed.partial), (vec!["time", "set"], ""));

        assert!(Typed::parse("say hi\nstop").is_none());
    }

    #[test]
    fn candidates_are_read_from_an_unbroken_reply() {
        let root = "/advancement (grant|revoke)/ban <targets> [<reason>]/xp -> experience";
        assert_eq!(
            candidates(root, "").literals,
            set(&["advancement", "ban", "xp"])
        );
        assert_eq!(alias(root, "xp"), Some("experience"));
        assert_eq!(alias(root, "ban"), None);

        let time = "/time set (<time>|day)/time add <time>/time pause";
        let found = candidates(time, "time");
        assert_eq!(found.literals, set(&["add", "pause", "set"]));
        assert!(found.arguments.is_empty());

        let found = candidates(time, "time set");
        assert_eq!(found.literals, set(&["day"]));
        assert_eq!(found.arguments, set(&["<time>"]));

        let optional = "/difficulty [peaceful|easy]/gamemode <gamemode> [<target>]";
        assert_eq!(
            candidates(optional, "difficulty").literals,
            set(&["easy", "peaceful"])
        );
        assert!(candidates("Unknown or incomplete command", "nope")
            .literals
            .is_empty());
    }
}
