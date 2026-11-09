//! RakNet Unconnected Ping: the status exchange Bedrock's server list uses,
//! one UDP datagram each way over the game port.

use std::time::Duration;

use anyhow::{bail, Context, Result};
use proto::server::ServerPingResult;
use tokio::net::UdpSocket;

use super::java::IO_TIMEOUT;

const PING_ID: u8 = 0x01;
const PONG_ID: u8 = 0x1c;
const OFFLINE_MAGIC: [u8; 16] = [
    0x00, 0xff, 0xff, 0x00, 0xfe, 0xfe, 0xfe, 0xfe, 0xfd, 0xfd, 0xfd, 0xfd, 0x12, 0x34, 0x56, 0x78,
];
const CLIENT_GUID: i64 = 0x4865_7374_6961;
const PONG_HEADER: usize = 1 + 8 + 8 + 16 + 2;
const MAX_DATAGRAM: usize = 2048;

pub async fn ping(port: u16) -> Result<ServerPingResult> {
    ping_within(port, IO_TIMEOUT)
        .await
        .context("cannot reach the server's game port")
}

async fn ping_within(port: u16, timeout: Duration) -> Result<ServerPingResult> {
    let socket = UdpSocket::bind("0.0.0.0:0")
        .await
        .context("cannot open a ping socket")?;
    socket.connect(("127.0.0.1", port)).await?;

    let mut request = Vec::with_capacity(33);
    request.push(PING_ID);
    request.extend_from_slice(&0i64.to_be_bytes());
    request.extend_from_slice(&OFFLINE_MAGIC);
    request.extend_from_slice(&CLIENT_GUID.to_be_bytes());
    socket.send(&request).await.context("ping write failed")?;

    let mut buf = [0u8; MAX_DATAGRAM];
    let len = tokio::time::timeout(timeout, socket.recv(&mut buf))
        .await
        .context("ping read timed out")?
        .context("ping read failed")?;
    parse_pong(&buf[..len])
}

fn parse_pong(datagram: &[u8]) -> Result<ServerPingResult> {
    if datagram.len() < PONG_HEADER {
        bail!("ping reply is truncated");
    }
    if datagram[0] != PONG_ID {
        bail!("unexpected ping packet id {}", datagram[0]);
    }
    if datagram[17..33] != OFFLINE_MAGIC {
        bail!("ping reply lacks the offline message magic");
    }
    let text_len = usize::from(u16::from_be_bytes([datagram[33], datagram[34]]));
    let text = datagram[PONG_HEADER..]
        .get(..text_len)
        .context("ping status string overruns its packet")?;
    let text = std::str::from_utf8(text).context("ping status is not UTF-8")?;

    let mut fields = text.split(';');
    if fields.next() != Some("MCPE") {
        bail!("ping status is not a Bedrock status");
    }
    let mut next = || fields.next().unwrap_or_default();
    let motd = next().to_string();
    let _protocol = next();
    let version = next().to_string();
    let count = |field: &str| field.parse().unwrap_or(0);
    Ok(ServerPingResult {
        motd,
        version,
        players_online: count(next()),
        players_max: count(next()),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pong(status: &str) -> Vec<u8> {
        let mut out = vec![PONG_ID];
        out.extend_from_slice(&1i64.to_be_bytes());
        out.extend_from_slice(&2i64.to_be_bytes());
        out.extend_from_slice(&OFFLINE_MAGIC);
        out.extend_from_slice(&(status.len() as u16).to_be_bytes());
        out.extend_from_slice(status.as_bytes());
        out
    }

    #[test]
    fn a_full_status_maps_onto_the_result() {
        let parsed = parse_pong(&pong(
            "MCPE;Dedicated Server;800;1.21.80;3;10;123;Bedrock level;Survival;1;19132;19133;",
        ))
        .unwrap();
        assert_eq!(parsed.motd, "Dedicated Server");
        assert_eq!(parsed.version, "1.21.80");
        assert_eq!(parsed.players_online, 3);
        assert_eq!(parsed.players_max, 10);
    }

    #[test]
    fn missing_trailing_fields_default() {
        let parsed = parse_pong(&pong("MCPE;Old;100;1.0.0")).unwrap();
        assert_eq!(parsed.motd, "Old");
        assert_eq!(parsed.players_online, 0);
        assert_eq!(parsed.players_max, 0);
    }

    #[test]
    fn a_malformed_pong_is_refused() {
        let good = pong("MCPE;a;1;1;0;1;");
        let mut wrong_id = good.clone();
        wrong_id[0] = 0x1d;
        let mut bad_magic = good.clone();
        bad_magic[20] ^= 0xff;
        let mut overrun = good.clone();
        overrun[33..35].copy_from_slice(&999u16.to_be_bytes());
        let mut not_utf8 = pong("MCPE;");
        not_utf8.push(0xff);
        let last = not_utf8.len() as u16 - PONG_HEADER as u16;
        not_utf8[33..35].copy_from_slice(&last.to_be_bytes());
        for datagram in [
            wrong_id,
            bad_magic,
            overrun,
            not_utf8,
            good[..10].to_vec(),
            pong("RAK;x;1;1;0;1;"),
        ] {
            assert!(parse_pong(&datagram).is_err());
        }
    }

    #[tokio::test]
    async fn a_ping_round_trips_over_udp() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        tokio::spawn(async move {
            let mut buf = [0u8; 64];
            let (len, peer) = server.recv_from(&mut buf).await.unwrap();
            assert_eq!(len, 33);
            assert_eq!(buf[0], PING_ID);
            assert_eq!(buf[9..25], OFFLINE_MAGIC);
            server
                .send_to(&pong("MCPE;Hi;1;1.21.80;2;8;"), peer)
                .await
                .unwrap();
        });
        let parsed = ping_within(port, Duration::from_secs(2)).await.unwrap();
        assert_eq!((parsed.motd.as_str(), parsed.players_online), ("Hi", 2));
    }

    #[tokio::test]
    async fn a_silent_server_times_out() {
        let server = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        let error = ping_within(port, Duration::from_millis(100)).await;
        assert!(error.unwrap_err().to_string().contains("timed out"));
    }
}
