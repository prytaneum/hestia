//! Every channel the daemon serves, wired onto the router. One `handle::<C>` per
//! channel, grouped into a registrar per domain; handlers reach the daemon's
//! collaborators through `HandlerContext` and return `ErrorInfo` for a typed
//! failure.

mod accounts;
mod backup;
mod cache;
mod config;
mod content;
mod entry;
mod files;
mod guards;
mod host;
mod instance;
mod java;
mod lifecycle;
mod modpack;
mod net;
mod players;
mod process;
mod screenshot;
mod server;
mod skins;
mod sync;
mod tasks;
mod transfer;
mod update;

use crate::runtime::{Channels, Router};

pub fn make_router() -> Router {
    let mut router = Router::default();
    let mut on = Channels::new(&mut router);

    lifecycle::register(&mut on);
    config::register(&mut on);
    cache::register(&mut on);
    java::register(&mut on);
    net::register(&mut on);
    host::register(&mut on);
    accounts::register(&mut on);
    skins::register(&mut on);
    process::register(&mut on);
    server::register(&mut on);
    files::register(&mut on);
    players::register(&mut on);
    tasks::register(&mut on);
    instance::register(&mut on);
    entry::register(&mut on);
    screenshot::register(&mut on);
    backup::register(&mut on);
    content::register(&mut on);
    modpack::register(&mut on);
    sync::register(&mut on);
    transfer::register(&mut on);
    update::register(&mut on);

    router
}
