//! Eclipse Temurin through the core Java roster: a runtime resolves and installs from a
//! recorded Adoptium index, with no network.

use std::io::Write;
use std::sync::Arc;

use crate::fixture;
use engine::{Adoptium, Cancel, Java};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

fn archive() -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    zip.start_file("jdk-21/bin/java", zip::write::SimpleFileOptions::default())
        .expect("entry");
    zip.write_all(b"#!/bin/sh\n").expect("body");
    zip.finish().expect("zip").into_inner()
}

async fn serve(archive: Vec<u8>) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let base = format!(
        "http://127.0.0.1:{}",
        listener.local_addr().expect("addr").port()
    );
    let sha = Sha256::digest(&archive)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let assets = format!(
        r#"[{{"release_name":"jdk-21.0.1+12","binary":{{"os":"linux","architecture":"x64","image_type":"jdk","package":{{"link":"{base}/jdk.zip","name":"jdk.zip","checksum":"{sha}","size":{}}}}}}}]"#,
        archive.len()
    );
    let index = r#"{"available_releases":[17,21],"available_lts_releases":[17,21]}"#;
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else {
                return;
            };
            let (assets, archive) = (assets.clone(), archive.clone());
            tokio::spawn(async move {
                let mut scratch = [0u8; 2048];
                let read = socket.read(&mut scratch).await.unwrap_or(0);
                let request = String::from_utf8_lossy(&scratch[..read]).to_string();
                let path = request.split_whitespace().nth(1).unwrap_or("/");
                let body: Vec<u8> = if path.starts_with("/v3/info/available_releases") {
                    index.as_bytes().to_vec()
                } else if path.starts_with("/v3/assets/latest/21/hotspot") {
                    assets.into_bytes()
                } else {
                    archive
                };
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(&body).await;
                let _ = socket.flush().await;
            });
        }
    });
    base
}

#[tokio::test]
async fn adoptium_installs_a_runtime_through_the_core_roster() {
    if !cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        return;
    }
    let home = tempfile::tempdir().expect("temp home");
    let base = serve(archive()).await;
    let java = Java::with_default(
        home.path().join("java"),
        Arc::new(Adoptium::at(base, fixture::loopback())),
    );

    let releases = java.releases("adoptium").await.expect("releases");
    assert_eq!(
        releases
            .iter()
            .map(|r| (r.major, r.lts))
            .collect::<Vec<_>>(),
        [(17, true), (21, true)]
    );
    assert!(java.download_size("adoptium", 21).await.expect("size") > 0);

    let outcome = java
        .install("adoptium", 21, false, &Cancel::new(), |_| {})
        .await
        .expect("install");
    assert_eq!(outcome.runtime.release_name, "jdk-21.0.1+12");
    assert!(outcome.runtime.executable.is_file());
    assert_eq!(java.download_size("adoptium", 21).await.expect("size"), 0);

    java.download_size("adoptium", 17)
        .await
        .expect_err("no build of 17 is published");
}
