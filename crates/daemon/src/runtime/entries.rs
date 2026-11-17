//! Naming entries the way a plugin's scope does: by stable id. A call or a
//! client names an entry by id or by name, so each is resolved through the
//! engine before the scope is checked or stored, and one that resolves to
//! nothing is refused.

use engine::Engine;
use proto::error::{EntryKind, EntryTarget, ErrorInfo};
use proto::plugin::EntryScope;

/// The stable id of the entry `target` names.
pub fn stable_id(engine: &Engine, target: &EntryTarget) -> Result<String, ErrorInfo> {
    let found = match target.kind {
        EntryKind::Server => engine.servers().get(&target.entry).map(|r| r.id),
        EntryKind::Instance => engine.instances().get(&target.entry).map(|r| r.id),
    };
    found.ok_or_else(|| ErrorInfo::EntryNotFound {
        entry: target.kind,
        reference: target.entry.clone(),
    })
}

/// `scope` with each reference replaced by the id of the server or instance it
/// names. A reference that names both, or neither, or a scope past
/// [`EntryScope::MAX_IDS`], is refused.
pub fn resolve_scope(engine: &Engine, scope: EntryScope) -> Result<EntryScope, ErrorInfo> {
    let scope = scope.normalised();
    if scope.all {
        return Ok(scope);
    }
    if scope.ids.len() > EntryScope::MAX_IDS {
        return Err(ErrorInfo::MalformedRequest {
            detail: format!(
                "an entries scope lists at most {} entries",
                EntryScope::MAX_IDS
            ),
        });
    }
    let ids = scope
        .ids
        .iter()
        .map(|reference| {
            let server = engine.servers().get(reference).map(|r| r.id);
            let instance = engine.instances().get(reference).map(|r| r.id);
            match (server, instance) {
                (Some(id), None) | (None, Some(id)) => Ok(id),
                (Some(_), Some(_)) => Err(ErrorInfo::MalformedRequest {
                    detail: format!("'{reference}' names a server and an instance; use its id"),
                }),
                (None, None) => Err(ErrorInfo::MalformedRequest {
                    detail: format!("no server or instance '{reference}'"),
                }),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(EntryScope { all: false, ids }.normalised())
}
