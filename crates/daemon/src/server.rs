//! The serving machinery: bind the endpoint, accept connections, and dispatch
//! each request through the router with a per-request context. The context
//! carries the connection's outbound channel, so streaming channels
//! (`events.subscribe`) are ordinary handlers.

use std::sync::Arc;

use ipc::protocol::{decode_request, encode_response, DecodeError};
use ipc::Connection;
use tracing::Instrument;

use engine::CallChain;

use crate::runtime::{Caller, HandlerContext, Router, RouterHost, Runtime};
use crate::services::make_router;

pub async fn run_daemon(log_path: std::path::PathBuf) -> i32 {
    let endpoint = ipc::endpoint::default_endpoint();
    let listener = match ipc::bind(&endpoint).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("cannot start: {e}");
            return 1;
        }
    };

    let runtime = Arc::new(Runtime::new(log_path));
    tracing::info!(
        version = common::app::VERSION,
        pid = std::process::id(),
        home = %runtime.engine().data_home().display(),
        "hestiad starting"
    );
    runtime.processes().recover();
    // Recovery's other half: a process can be re-adopted, but a job cannot —
    // so its leftovers (temp artifacts, a record still mid-create) are settled
    // before anything else writes.
    runtime.engine().recover();
    runtime.engine().reconcile_idle();
    runtime.engine().reconcile_idle_packs().await;
    crate::runtime::spawn_task_scheduler(runtime.clone());
    crate::runtime::spawn_metrics_sampler(runtime.clone());
    crate::runtime::spawn_network_watcher(runtime.clone());
    let presence = crate::runtime::spawn_presence_updater(runtime.clone());
    let router = Arc::new(make_router());
    if let Err(error) = runtime
        .plugins()
        .attach(Arc::new(RouterHost::new(router.clone(), &runtime)))
    {
        tracing::error!(%error, "plugins are unavailable");
    }
    let plugins = runtime.plugins().clone();
    tokio::spawn(async move { plugins.load_enabled().await });

    tracing::info!("hestiad listening on {}", endpoint.display());
    crate::tray::spawn();
    tokio::select! {
        _ = accept_loop(listener, router, runtime.clone()) => {}
        _ = runtime.stopped() => tracing::info!("stop requested"),
        _ = shutdown_signal() => tracing::info!("signal received"),
    }
    runtime.plugins().shutdown();
    runtime.shutdown_workloads().await;
    // A game session outlives this process, but the presence connection does
    // not — left alone, Discord keeps showing whatever was published last.
    if let Some(presence) = presence {
        let _ = tokio::task::spawn_blocking(move || presence.shutdown()).await;
    }
    tracing::info!("hestiad stopped");
    0
}

async fn accept_loop(listener: ipc::Listener, router: Arc<Router>, runtime: Arc<Runtime>) {
    loop {
        let (conn, peer) = match listener.accept().await {
            Ok(pair) => pair,
            Err(e) => {
                tracing::warn!("accept failed: {e}");
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                continue;
            }
        };
        if !peer.authorized() {
            tracing::warn!(
                uid = peer.uid,
                "rejecting connection from unauthorized peer"
            );
            continue;
        }
        let router = router.clone();
        let runtime = runtime.clone();
        let conn_id = runtime.hub().next_subscriber_id();
        // A span per connection, so every request and handler log it fans out is
        // tagged with the connection id and peer uid — the traceability seam.
        let span = tracing::info_span!("conn", id = conn_id, uid = peer.uid);
        tokio::spawn(
            async move {
                tracing::debug!("client connected");
                serve_connection(conn, conn_id, router, runtime).await;
                tracing::debug!("client disconnected");
            }
            .instrument(span),
        );
    }
}

async fn serve_connection(
    conn: Connection,
    conn_id: u64,
    router: Arc<Router>,
    runtime: Arc<Runtime>,
) {
    let (mut reader, mut writer) = conn.into_split();
    let (out_tx, mut out_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let writer_task = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            if writer.send(&frame).await.is_err() {
                break;
            }
        }
    });

    // Requests on one connection are dispatched concurrently: a client
    // multiplexes replies by id, so a slow handler must not head-of-line block
    // the rest. This matters most for the desktop, which funnels every call
    // over a single shared connection — a long launch/materialize would
    // otherwise stall every other request and freeze the UI. Each handler owns
    // its own `out` clone and replies out of order through the writer task.
    while let Ok(Some(frame)) = reader.recv().await {
        let req = match decode_request(&frame) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!("rejecting frame: {e}");
                let info = match e {
                    DecodeError::IncompatibleVersion { got, want } => {
                        proto::error::ErrorInfo::IncompatibleVersion { got, want }
                    }
                    DecodeError::Malformed(detail) => {
                        proto::error::ErrorInfo::MalformedRequest { detail }
                    }
                };
                let _ = out_tx.send(encode_response(&crate::runtime::error_response(info)));
                continue;
            }
        };
        let out = out_tx.clone();
        let ctx = HandlerContext {
            runtime: runtime.clone(),
            conn_id,
            out: out_tx.clone(),
            caller: Caller::Client,
            chain: CallChain::default(),
        };
        let router = router.clone();
        tokio::spawn(async move {
            let id = req.id;
            let mut res = router.route(req, ctx).await;
            res.id = id;
            let _ = out.send(encode_response(&res));
        });
    }

    runtime.hub().unsubscribe(conn_id);
    drop(out_tx);
    let _ = writer_task.await;
}

#[cfg(unix)]
async fn shutdown_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = term.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
}
