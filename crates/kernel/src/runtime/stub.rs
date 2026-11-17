use std::collections::HashMap;
use wasmtime::component::types::ComponentItem;
use wasmtime::component::{Component, Linker, Type, Val};
use wasmtime::Engine;

use super::error::SandboxError;
use super::host::State;

/// Interface name to the permission whose absence replaces it with a stub.
pub type Denials = HashMap<String, String>;

/// Replaces each denied import with functions that answer `denied(permission)`.
/// Only an interface whose every item is a function returning a `result` can be
/// denied: anything else would have to trap, and traps count toward the
/// plugin's limit, or leave a host resource linked. In developer mode each
/// denial also names the function in the plugin log.
pub(super) fn link_denials(
    linker: &mut Linker<State>,
    engine: &Engine,
    component: &Component,
    denials: &Denials,
) -> Result<(), SandboxError> {
    linker.allow_shadowing(true);
    for (name, import) in component.component_type().imports(engine) {
        let (Some(permission), ComponentItem::ComponentInstance(interface)) =
            (denials.get(name), import.ty)
        else {
            continue;
        };
        let undeniable = |item: &str| {
            SandboxError::Load(wasmtime::format_err!(
                "'{name}' cannot be denied: '{item}' has no error to answer with"
            ))
        };
        let mut stub = linker.instance(name).map_err(SandboxError::Load)?;
        for (function, item) in interface.exports(engine) {
            let signature = match item.ty {
                ComponentItem::ComponentFunc(signature) => signature,
                ComponentItem::Type(_) => continue,
                _ => return Err(undeniable(function)),
            };
            if !matches!(signature.results().next(), Some(Type::Result(_))) {
                return Err(undeniable(function));
            }
            let permission = permission.clone();
            let interface = name.split('@').next().unwrap_or(name).to_owned();
            let function_name = function.to_owned();
            stub.func_new(function, move |context, _, _, results| {
                context
                    .data()
                    .log
                    .denied(&interface, &function_name, &permission);
                let denied = Val::Variant(
                    "denied".into(),
                    Some(Box::new(Val::String(permission.clone()))),
                );
                results[0] = Val::Result(Err(Some(Box::new(denied))));
                Ok(())
            })
            .map_err(SandboxError::Load)?;
        }
    }
    Ok(())
}
