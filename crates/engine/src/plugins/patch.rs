//! What a hook may add to a launch. An allowlist, not a denylist: anything not
//! named here is dropped with a warning, so a new JVM option is refused until
//! someone decides it is safe. Hooks are customisation, never a control
//! ([0095](../../../../docs/decisions/0095-interceptors-are-bounded-and-fail-open.md)),
//! but a plugin that may only tune the heap must not be able to load code.

use std::collections::BTreeSet;
use std::path::PathBuf;

use proto::plugin::is_launch_property;
use proto::warning::WarningInfo;

const SHOWN_CHARS: usize = 120;
const VALUE_CHARS: usize = 1024;

const FLAG_OPTIONS: [&str; 8] = [
    "UseG1GC",
    "UseZGC",
    "UseParallelGC",
    "ZGenerational",
    "UnlockExperimentalVMOptions",
    "AlwaysPreTouch",
    "ParallelRefProcEnabled",
    "DisableExplicitGC",
];

#[derive(Clone, Copy)]
enum Shape {
    Number,
    Size,
}

const VALUE_OPTIONS: [(&str, Shape); 11] = [
    ("MaxGCPauseMillis", Shape::Number),
    ("G1HeapRegionSize", Shape::Size),
    ("G1NewSizePercent", Shape::Number),
    ("G1ReservePercent", Shape::Number),
    ("G1MixedGCCountTarget", Shape::Number),
    ("InitiatingHeapOccupancyPercent", Shape::Number),
    ("ParallelGCThreads", Shape::Number),
    ("ConcGCThreads", Shape::Number),
    ("MetaspaceSize", Shape::Size),
    ("MaxMetaspaceSize", Shape::Size),
    ("ReservedCodeCacheSize", Shape::Size),
];

/// A hook's answer after the allowlist: what stays, and a warning for each
/// argument or wrapper that did not.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct LaunchPatch {
    pub jvm_args: Vec<String>,
    pub game_args: Vec<String>,
    pub wrappers: Vec<PathBuf>,
    pub dropped: Vec<WarningInfo>,
}

impl LaunchPatch {
    /// Keeps what `plugin` may set. `properties` is the `-D` keys its manifest
    /// declared; a key on the hard deny list is refused even when declared.
    pub fn vetted(
        plugin: &str,
        properties: &BTreeSet<String>,
        jvm_args: Vec<String>,
        game_args: Vec<String>,
    ) -> LaunchPatch {
        let mut patch = LaunchPatch::default();
        for arg in jvm_args {
            if jvm_allowed(&arg, properties) {
                patch.jvm_args.push(arg);
            } else {
                patch.drop_arg(plugin, &arg);
            }
        }
        let mut rest = game_args.into_iter().peekable();
        while let Some(arg) = rest.next() {
            match arg.as_str() {
                "--fullscreen" => patch.game_args.push(arg),
                "--width" | "--height" if rest.peek().is_some_and(|next| is_number(next)) => {
                    patch.game_args.push(arg);
                    patch.game_args.extend(rest.next());
                }
                _ => patch.drop_arg(plugin, &arg),
            }
        }
        patch
    }

    /// [`vetted`](Self::vetted) for a server start: the window arguments mean
    /// nothing to a server, so every game argument is dropped with a warning.
    pub fn vetted_for_server(
        plugin: &str,
        properties: &BTreeSet<String>,
        jvm_args: Vec<String>,
        game_args: Vec<String>,
    ) -> LaunchPatch {
        let mut patch = Self::vetted(plugin, properties, jvm_args, Vec::new());
        for arg in game_args {
            patch.drop_arg(plugin, &arg);
        }
        patch
    }

    /// Keeps `requested` when `resolve` finds the pinned path of that program;
    /// otherwise drops it with a warning.
    pub fn with_wrapper(
        mut self,
        plugin: &str,
        requested: Option<String>,
        resolve: impl FnOnce(&str) -> Option<PathBuf>,
    ) -> LaunchPatch {
        if let Some(name) = requested {
            match resolve(&name) {
                Some(path) => self.wrappers.push(path),
                None => self.drop_arg(plugin, &name),
            }
        }
        self
    }

    fn drop_arg(&mut self, plugin: &str, arg: &str) {
        tracing::warn!(plugin, arg = %shown(arg), "launch argument dropped");
        self.dropped.push(WarningInfo::LaunchArgDropped {
            plugin: plugin.to_string(),
            arg: shown(arg),
        });
    }

    /// Appends `next` after `self`, so plugins fold in the order they ran; the
    /// first plugin's wrapper is the outermost.
    pub fn then(mut self, next: LaunchPatch) -> LaunchPatch {
        self.jvm_args.extend(next.jvm_args);
        self.game_args.extend(next.game_args);
        self.wrappers.extend(next.wrappers);
        self.dropped.extend(next.dropped);
        self
    }
}

fn shown(arg: &str) -> String {
    arg.chars().take(SHOWN_CHARS).collect()
}

fn jvm_allowed(arg: &str, properties: &BTreeSet<String>) -> bool {
    if let Some(option) = arg.strip_prefix("-XX:") {
        return xx_allowed(option);
    }
    let Some((key, value)) = arg.strip_prefix("-D").and_then(|rest| rest.split_once('=')) else {
        return false;
    };
    properties.contains(key)
        && is_launch_property(key)
        && value.len() <= VALUE_CHARS
        && !value.chars().any(char::is_control)
}

fn xx_allowed(option: &str) -> bool {
    if let Some(name) = option.strip_prefix(['+', '-']) {
        return FLAG_OPTIONS.contains(&name);
    }
    let Some((name, value)) = option.split_once('=') else {
        return false;
    };
    VALUE_OPTIONS
        .iter()
        .find(|(known, _)| *known == name)
        .is_some_and(|(_, shape)| match shape {
            Shape::Number => is_number(value),
            Shape::Size => is_size(value),
        })
}

fn is_number(value: &str) -> bool {
    (1..=9).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit())
}

fn is_size(value: &str) -> bool {
    is_number(
        value
            .strip_suffix(['k', 'K', 'm', 'M', 'g', 'G'])
            .unwrap_or(value),
    )
}

#[cfg(test)]
mod tests;
