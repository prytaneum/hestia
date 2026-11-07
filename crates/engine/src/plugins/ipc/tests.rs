use super::*;

fn connections(permissions: &[Permission], ipc: &[&str]) -> Connections {
    Connections::new(
        "talker".into(),
        Arc::new(Grant {
            permissions: permissions.iter().copied().collect(),
            ipc: ipc.iter().map(|pattern| pattern.to_string()).collect(),
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

#[tokio::test]
async fn an_unknown_connection_is_not_found() {
    let ipc = connections(&[Permission::IpcConnect], &[]);
    assert!(matches!(ipc.close("7"), Err(IpcError::NotFound(_))));
    assert!(matches!(
        ipc.receive("7", 16, Duration::from_millis(10)).await,
        Err(IpcError::NotFound(_))
    ));
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

#[cfg(unix)]
#[tokio::test]
async fn nothing_listening_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let dialed = platform::dial_in(&[dir.path().into()], "chat-ipc-0").await;
    assert!(matches!(dialed, Err(IpcError::NotFound(_))));
}

#[tokio::test]
async fn a_quiet_peer_reads_as_empty_after_the_wait() {
    let ipc = connections(&[Permission::IpcConnect], &[]);
    let (near, _far) = tokio::io::duplex(64);
    let id = ipc.adopt(Box::new(near));
    let read = ipc
        .receive(&id, 16, Duration::from_millis(20))
        .await
        .unwrap();
    assert!(read.is_empty());
}
