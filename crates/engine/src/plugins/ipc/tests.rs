use proto::plugin::PermissionScope;

use super::*;
use tokio::io::AsyncReadExt;

fn connections(permissions: &[Permission], ipc: &[&str]) -> Connections {
    Connections::new(
        "talker".into(),
        Arc::new(Grant {
            permissions: permissions
                .iter()
                .map(|permission| {
                    (
                        *permission,
                        PermissionScope::of(ScopeKind::Endpoints, ipc.iter().copied()),
                    )
                })
                .collect(),
            ..Grant::default()
        }),
    )
}

#[tokio::test]
async fn without_the_permission_every_call_is_denied() {
    let ipc = connections(&[], &["chat-ipc-*"]);
    assert!(matches!(
        ipc.open("chat-ipc-0").await,
        Err(IpcError::Denied(_))
    ));
    assert!(matches!(ipc.list(), Err(IpcError::Denied(_))));
    assert!(matches!(
        ipc.send("1", b"x").await,
        Err(IpcError::Denied(_))
    ));
}

#[tokio::test]
async fn an_endpoint_the_grant_does_not_name_is_refused_before_dialing() {
    let ipc = connections(&[Permission::IpcConnect], &["chat-ipc-*"]);
    for endpoint in [
        "docker.sock",
        "chat-ipc-",
        "../chat-ipc-0",
        "chat-ipc-0/x",
        "chat-ipc-*",
    ] {
        assert!(
            matches!(ipc.open(endpoint).await, Err(IpcError::Denied(_))),
            "{endpoint}"
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn a_connection_round_trips_and_outlives_the_call_that_opened_it() {
    use tokio::net::UnixListener;

    let dir = tempfile::tempdir().unwrap();
    let listener = UnixListener::bind(dir.path().join("chat-ipc-0")).unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut buffer = [0; 5];
        stream.read_exact(&mut buffer).await.unwrap();
        stream.write_all(b"pong").await.unwrap();
        buffer
    });

    let ipc = connections(&[Permission::IpcConnect], &["chat-ipc-*"]);
    let stream = platform::dial_in(&[dir.path().into()], "chat-ipc-0")
        .await
        .unwrap();
    let id = ipc.adopt(stream);
    assert_eq!(ipc.list().unwrap(), std::slice::from_ref(&id));

    ipc.send(&id, b"ping!").await.unwrap();
    assert_eq!(&peer.await.unwrap(), b"ping!");
    let reply = ipc.receive(&id, 64, Duration::from_secs(5)).await.unwrap();
    assert_eq!(reply, b"pong");

    let closed = ipc.receive(&id, 64, Duration::from_secs(5)).await;
    assert!(matches!(closed, Err(IpcError::NotFound(_))), "{closed:?}");
    assert!(ipc.list().unwrap().is_empty());
}

mod pushed {
    use proto::pushed::{IpcClosedEvent, IpcReadableEvent};
    use proto::Topic;
    use tokio::io::{AsyncWriteExt, DuplexStream};
    use tokio::sync::mpsc::UnboundedReceiver;

    use super::*;

    const QUIET: Duration = Duration::from_millis(100);

    fn open() -> (Connections, UnboundedReceiver<Pushed>, String, DuplexStream) {
        let ipc = connections(&[Permission::IpcConnect], &[]);
        let events = ipc.pushed();
        let (near, far) = tokio::io::duplex(4096);
        let id = ipc.adopt(Box::new(near));
        (ipc, events, id, far)
    }

    async fn next(events: &mut UnboundedReceiver<Pushed>) -> Option<Pushed> {
        tokio::time::timeout(QUIET, events.recv())
            .await
            .ok()
            .flatten()
    }

    fn readable(id: &str) -> Pushed {
        Pushed {
            topic: IpcReadableEvent::TOPIC,
            payload: serde_json::json!({ "connection": id }),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn arriving_bytes_fire_one_readable_event_until_the_plugin_receives() {
        let (ipc, mut events, id, mut far) = open();
        far.write_all(b"one").await.unwrap();
        far.write_all(b"two").await.unwrap();
        assert_eq!(next(&mut events).await, Some(readable(&id)));
        far.write_all(b"three").await.unwrap();
        assert_eq!(next(&mut events).await, None);

        let read = ipc.receive(&id, 64, Duration::ZERO).await.unwrap();
        assert_eq!(read, b"onetwothree");
        assert_eq!(next(&mut events).await, None);

        far.write_all(b"four").await.unwrap();
        assert_eq!(next(&mut events).await, Some(readable(&id)));
    }

    #[tokio::test(start_paused = true)]
    async fn a_receive_that_leaves_bytes_behind_fires_again() {
        let (ipc, mut events, id, mut far) = open();
        far.write_all(b"abcdef").await.unwrap();
        assert_eq!(next(&mut events).await, Some(readable(&id)));

        assert_eq!(ipc.receive(&id, 4, Duration::ZERO).await.unwrap(), b"abcd");
        assert_eq!(next(&mut events).await, Some(readable(&id)));
        assert_eq!(ipc.receive(&id, 4, Duration::ZERO).await.unwrap(), b"ef");
        assert_eq!(next(&mut events).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn bytes_a_waiting_receive_takes_fire_nothing() {
        let (ipc, mut events, id, mut far) = open();
        let waiting =
            tokio::spawn(
                async move { ipc.receive(&id, 64, Duration::from_secs(5)).await.unwrap() },
            );
        tokio::task::yield_now().await;
        far.write_all(b"hi").await.unwrap();
        assert_eq!(waiting.await.unwrap(), b"hi");
        assert_eq!(next(&mut events).await, None);
    }

    #[tokio::test(start_paused = true)]
    async fn a_peer_that_closes_fires_closed_once_after_its_last_bytes() {
        let (ipc, mut events, id, mut far) = open();
        far.write_all(b"bye").await.unwrap();
        drop(far);
        assert_eq!(next(&mut events).await, Some(readable(&id)));
        assert_eq!(
            next(&mut events).await,
            Some(Pushed {
                topic: IpcClosedEvent::TOPIC,
                payload: serde_json::json!({ "connection": id }),
            })
        );
        assert_eq!(next(&mut events).await, None);

        assert_eq!(ipc.receive(&id, 64, Duration::ZERO).await.unwrap(), b"bye");
        assert!(matches!(
            ipc.receive(&id, 64, Duration::ZERO).await,
            Err(IpcError::NotFound(_))
        ));
        assert_eq!(next(&mut events).await, None);
    }
}
