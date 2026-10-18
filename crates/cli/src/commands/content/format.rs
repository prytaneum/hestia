//! Label and number formatting shared by the browse and management views.

use client::proto::content::{
    ContentKind, ContentVersion, InstalledContent, ReleaseChannel, SideSupport,
};

use crate::ui::components::{Picker, PickerItem};

pub(super) fn source_label(item: &InstalledContent) -> String {
    if item.project_id.is_empty() {
        item.source.clone()
    } else {
        format!("{} ({})", item.source, item.version_number)
    }
}

pub(super) fn side_label(side: SideSupport) -> String {
    match side {
        SideSupport::Required => "required",
        SideSupport::Optional => "optional",
        SideSupport::Unsupported => "unsupported",
        SideSupport::Unknown => "unknown",
    }
    .to_string()
}

pub(super) fn channel_label(channel: ReleaseChannel) -> &'static str {
    match channel {
        ReleaseChannel::Release => "release",
        ReleaseChannel::Beta => "beta",
        ReleaseChannel::Alpha => "alpha",
    }
}

pub fn kind_plural(kind: ContentKind) -> &'static str {
    match kind {
        ContentKind::Mod => "mods",
        ContentKind::Modpack => "modpacks",
        ContentKind::ResourcePack => "resourcepacks",
        ContentKind::Shader => "shaders",
        ContentKind::DataPack => "datapacks",
        ContentKind::Plugin => "plugins",
    }
}

/// A large count in compact units (180204729 → "180.2M").
pub(crate) fn compact(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

pub(super) fn truncate(text: &str, max: usize) -> String {
    let flat = text.replace('\n', " ");
    if flat.chars().count() <= max {
        return flat;
    }
    let mut out: String = flat.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// A world's folder name — the last component of the stored path
/// (`saves/<name>` for an instance, the level-name dir for a server), which is
/// also how the world pickers and `Target::worlds` name it. `None` for content
/// with no world.
pub(super) fn world_name(world: &str) -> Option<&str> {
    let name = world.rsplit('/').next().unwrap_or(world);
    (!name.is_empty()).then_some(name)
}

/// How an item's world targeting reads in a listing: the folder names it
/// mirrors into, or `all` for a datapack that follows every world.
pub(super) fn world_label(item: &InstalledContent) -> String {
    if item.kind != ContentKind::DataPack {
        return "-".to_string();
    }
    match item.worlds.is_empty() {
        true => "all".to_string(),
        false => item
            .worlds
            .iter()
            .filter_map(|w| world_name(w))
            .collect::<Vec<_>>()
            .join(", "),
    }
}

/// Build a searchable version picker over a project's versions, paired with the
/// versions themselves so the selected index maps back to one.
pub(super) fn version_picker(versions: &[ContentVersion]) -> (Picker, Vec<ContentVersion>) {
    let items: Vec<PickerItem> = versions
        .iter()
        .map(|v| PickerItem {
            label: v.version_number.clone(),
            tag: format!(
                "{} · {}",
                channel_label(v.channel),
                v.game_versions.join(", ")
            ),
            stable: v.channel == ReleaseChannel::Release,
        })
        .collect();
    (Picker::new(items), versions.to_vec())
}
