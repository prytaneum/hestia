//! Minimal RCON client for the vanilla server's remote console. RCON is the
//! command channel by design: it is re-establishable TCP state, so it works
//! for adopted processes and across daemon restarts, where a stdin pipe would
//! not (see the supervisor's workloads-outlive-the-daemon decision).
//!
//! Wire format (little-endian): `[length:i32][id:i32][type:i32][body][\0\0]`
//! where `length` counts everything after itself. Types: 3 login, 2 command,
//! 0 response. A login reply echoing id -1 means the password was rejected.
//!
//! A long reply spans packets with no end marker; a probe of an unknown type ends it.

use std::io::ErrorKind;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const LOGIN: i32 = 3;
const COMMAND: i32 = 2;
const PROBE: i32 = 100;
const IO_TIMEOUT: Duration = Duration::from_secs(5);
// Vanilla packets carry at most 4096 UTF-16 units, three bytes apiece at worst.
const MAX_BODY: usize = 4096 * 3;

pub struct Rcon {
    stream: TcpStream,
    next_id: i32,
}

impl Rcon {
    pub async fn connect(port: u16, password: &str) -> Result<Rcon> {
        let stream = tokio::time::timeout(IO_TIMEOUT, TcpStream::connect(("127.0.0.1", port)))
            .await
            .context("rcon connect timed out")?
            .context("cannot reach the server's rcon port")?;
        let mut rcon = Rcon { stream, next_id: 0 };
        let id = rcon.send(LOGIN, password).await?;
        let (reply_id, _) = rcon.recv().await?;
        if reply_id != id {
            bail!("rcon authentication failed (wrong password?)");
        }
        Ok(rcon)
    }

    pub async fn command(&mut self, command: &str) -> Result<String> {
        let id = self.send(COMMAND, command).await?;
        let (reply_id, mut reply) = self.recv().await?;
        if reply_id != id {
            bail!("rcon response out of sequence");
        }
        // Sent only now: the server drops a connection whose read holds two packets.
        let probe = self.send(PROBE, "").await?;
        loop {
            let (reply_id, body) = self.recv().await?;
            if reply_id == probe {
                return Ok(reply);
            }
            if reply_id != id {
                bail!("rcon response out of sequence");
            }
            reply.push_str(&body);
        }
    }

    pub fn is_open(&self) -> bool {
        matches!(
            self.stream.try_read(&mut [0u8; 1]),
            Err(error) if error.kind() == ErrorKind::WouldBlock
        )
    }

    async fn send(&mut self, packet_type: i32, body: &str) -> Result<i32> {
        self.next_id += 1;
        let id = self.next_id;
        let payload_len = 4 + 4 + body.len() + 2;
        let mut frame = Vec::with_capacity(4 + payload_len);
        frame.extend_from_slice(&(payload_len as i32).to_le_bytes());
        frame.extend_from_slice(&id.to_le_bytes());
        frame.extend_from_slice(&packet_type.to_le_bytes());
        frame.extend_from_slice(body.as_bytes());
        frame.extend_from_slice(&[0, 0]);
        tokio::time::timeout(IO_TIMEOUT, self.stream.write_all(&frame))
            .await
            .context("rcon write timed out")?
            .context("rcon write failed")?;
        Ok(id)
    }

    async fn recv(&mut self) -> Result<(i32, String)> {
        tokio::time::timeout(IO_TIMEOUT, self.recv_inner())
            .await
            .context("rcon read timed out")?
    }

    async fn recv_inner(&mut self) -> Result<(i32, String)> {
        let mut header = [0u8; 4];
        self.stream
            .read_exact(&mut header)
            .await
            .context("rcon connection closed")?;
        let length = i32::from_le_bytes(header);
        if !(10..=(MAX_BODY as i32 + 10)).contains(&length) {
            bail!("malformed rcon packet (length {length})");
        }
        let mut payload = vec![0u8; length as usize];
        self.stream
            .read_exact(&mut payload)
            .await
            .context("rcon connection closed mid-packet")?;
        let id = i32::from_le_bytes(payload[0..4].try_into().unwrap());
        let body = &payload[8..payload.len().saturating_sub(2)];
        Ok((id, String::from_utf8_lossy(body).into_owned()))
    }
}
