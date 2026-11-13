//! The one parser of a Maven coordinate, `group:artifact:version[:classifier][@ext]`,
//! and the repository layout it names. Core flavors and a plugin's libraries
//! derive their paths and URLs from it.

use std::fmt;

const DEFAULT_EXTENSION: &str = "jar";

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CoordinateError {
    #[error("`{0}` is not group:artifact:version[:classifier][@ext]")]
    Shape(String),
    #[error("`{part}` in `{coordinate}` holds a character a Maven name does not")]
    Character { coordinate: String, part: String },
}

/// A parsed coordinate. Every part is limited to letters, digits and `.`, `_`,
/// `+`, `-`, and none is made only of dots, so no part can name another
/// directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coordinate {
    pub group: String,
    pub artifact: String,
    pub version: String,
    pub classifier: Option<String>,
    pub extension: String,
}

impl Coordinate {
    pub fn parse(coordinate: &str) -> Result<Self, CoordinateError> {
        let shape = || CoordinateError::Shape(coordinate.to_string());
        let (parts, extension) = coordinate
            .split_once('@')
            .map_or((coordinate, DEFAULT_EXTENSION), |(parts, extension)| {
                (parts, extension)
            });
        let parts: Vec<&str> = parts.split(':').collect();
        if !(3..=4).contains(&parts.len()) {
            return Err(shape());
        }
        for part in parts.iter().chain([&extension]) {
            if part.is_empty() {
                return Err(shape());
            }
            let plain = part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-'));
            if !plain || part.chars().all(|c| c == '.') {
                return Err(CoordinateError::Character {
                    coordinate: coordinate.to_string(),
                    part: part.to_string(),
                });
            }
        }
        Ok(Coordinate {
            group: parts[0].to_string(),
            artifact: parts[1].to_string(),
            version: parts[2].to_string(),
            classifier: parts.get(3).map(|classifier| classifier.to_string()),
            extension: extension.to_string(),
        })
    }

    pub fn filename(&self) -> String {
        let classifier = self
            .classifier
            .as_ref()
            .map(|classifier| format!("-{classifier}"))
            .unwrap_or_default();
        format!(
            "{}-{}{classifier}.{}",
            self.artifact, self.version, self.extension
        )
    }

    /// The path under a repository root: `group/as/dirs/artifact/version/file`.
    pub fn path(&self) -> String {
        format!(
            "{}/{}/{}/{}",
            self.group.replace('.', "/"),
            self.artifact,
            self.version,
            self.filename()
        )
    }

    /// The URL of the file in the repository at `repository`.
    pub fn url_in(&self, repository: &str) -> String {
        format!("{}/{}", repository.trim_end_matches('/'), self.path())
    }
}

impl fmt::Display for Coordinate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}:{}", self.group, self.artifact, self.version)?;
        if let Some(classifier) = &self.classifier {
            write!(f, ":{classifier}")?;
        }
        if self.extension != DEFAULT_EXTENSION {
            write!(f, "@{}", self.extension)?;
        }
        Ok(())
    }
}

/// The repository-relative path of `coordinate`, or none when it does not parse.
pub fn path(coordinate: &str) -> Option<String> {
    Coordinate::parse(coordinate)
        .ok()
        .map(|parsed| parsed.path())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_coordinate_becomes_a_jar_path() {
        let coordinate = Coordinate::parse("net.fabricmc:fabric-loader:0.16.9").unwrap();
        assert_eq!(
            coordinate.path(),
            "net/fabricmc/fabric-loader/0.16.9/fabric-loader-0.16.9.jar"
        );
        assert_eq!(coordinate.filename(), "fabric-loader-0.16.9.jar");
    }

    #[test]
    fn a_classifier_and_an_extension_land_in_the_filename() {
        let coordinate = Coordinate::parse("net.neoforged:neoforge:21.1.1:installer@zip").unwrap();
        assert_eq!(
            coordinate.path(),
            "net/neoforged/neoforge/21.1.1/neoforge-21.1.1-installer.zip"
        );
        let natives = Coordinate::parse("org.lwjgl:lwjgl:3.3.3:natives-linux-arm64").unwrap();
        assert_eq!(natives.classifier.as_deref(), Some("natives-linux-arm64"));
        assert_eq!(natives.extension, "jar");
        let bare = Coordinate::parse("a:b:1@tar.gz").unwrap();
        assert_eq!(bare.path(), "a/b/1/b-1.tar.gz");
    }

    #[test]
    fn a_version_keeps_its_suffix_and_a_snapshot_is_not_special() {
        let coordinate = Coordinate::parse("org.example:lib:1.0.0-beta.1+build_7").unwrap();
        assert_eq!(coordinate.filename(), "lib-1.0.0-beta.1+build_7.jar");
        let snapshot = Coordinate::parse("org.example:lib:1.0-SNAPSHOT").unwrap();
        assert_eq!(snapshot.filename(), "lib-1.0-SNAPSHOT.jar");
    }

    #[test]
    fn a_repository_url_joins_with_one_slash() {
        let coordinate = Coordinate::parse("a.b:c:1").unwrap();
        assert_eq!(
            coordinate.url_in("https://repo.example.com/maven"),
            "https://repo.example.com/maven/a/b/c/1/c-1.jar"
        );
        assert_eq!(
            coordinate.url_in("https://repo.example.com/maven///"),
            "https://repo.example.com/maven/a/b/c/1/c-1.jar"
        );
    }

    #[test]
    fn the_coordinate_reads_back_as_it_was_written() {
        for text in [
            "a.b:c:1",
            "a.b:c:1:cls",
            "a.b:c:1@zip",
            "a.b:c:1:cls@tar.gz",
        ] {
            assert_eq!(Coordinate::parse(text).unwrap().to_string(), text);
        }
    }

    #[test]
    fn a_malformed_coordinate_is_refused() {
        for bad in [
            "",
            "a:b",
            "a:b:",
            ":b:1",
            "a:b:1:c:d",
            "a:b:1@",
            "a:b:..",
            "a:..:1",
            "../x:b:1",
            "a/b:c:1",
            "a:b\\c:1",
            "a:b:1 ",
            "a:b:1:cls/../x",
            "[a:b:1]",
            "a:b:${v}",
        ] {
            assert!(Coordinate::parse(bad).is_err(), "{bad:?}");
            assert_eq!(path(bad), None);
        }
    }
}
