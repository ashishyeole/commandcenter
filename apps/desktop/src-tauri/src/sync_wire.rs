//! Length-bounded Noise transport. No HTTP endpoint, cookies, or renderer network access.
use crate::Result;
use serde::{de::DeserializeOwned, Serialize};
use snow::{HandshakeState, TransportState};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};
use zeroize::Zeroizing;
const FRAME: usize = 65535;
// A single 10 MiB attachment event plus its causal base; encrypted frames stay 60 KB.
const MESSAGE: usize = 32 * 1024 * 1024;
async fn write_frame(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    if data.is_empty() || data.len() > FRAME {
        return Err("Invalid frame size".into());
    }
    stream
        .write_u32(data.len() as u32)
        .await
        .map_err(|_| "Peer write failed")?;
    stream
        .write_all(data)
        .await
        .map_err(|_| "Peer write failed".into())
}
async fn read_frame(stream: &mut TcpStream, limit: usize) -> Result<Vec<u8>> {
    let n = stream.read_u32().await.map_err(|_| "Peer disconnected")? as usize;
    if n == 0 || n > limit {
        return Err("Peer frame exceeds limit".into());
    }
    let mut bytes = vec![0; n];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|_| "Incomplete peer frame")?;
    Ok(bytes)
}
pub async fn write_prelude<T: Serialize>(stream: &mut TcpStream, value: &T) -> Result<()> {
    write_frame(
        stream,
        &serde_json::to_vec(value).map_err(|e| e.to_string())?,
    )
    .await
}
pub async fn read_prelude<T: DeserializeOwned>(stream: &mut TcpStream) -> Result<T> {
    serde_json::from_slice(&read_frame(stream, 4096).await?)
        .map_err(|_| "Invalid protocol prelude".into())
}
pub struct Channel {
    stream: TcpStream,
    noise: TransportState,
    pub remote_key: Vec<u8>,
    pub verification: String,
}
pub async fn handshake(
    mut stream: TcpStream,
    mut noise: HandshakeState,
    initiator: bool,
) -> Result<Channel> {
    tokio::time::timeout(Duration::from_secs(15), async move {
        let mut write = initiator;
        let mut buffer = vec![0; FRAME];
        while !noise.is_handshake_finished() {
            if write {
                let n = noise
                    .write_message(&[], &mut buffer)
                    .map_err(|_| "Noise handshake failed")?;
                write_frame(&mut stream, &buffer[..n]).await?;
            } else {
                let frame = read_frame(&mut stream, FRAME).await?;
                let n = noise
                    .read_message(&frame, &mut buffer)
                    .map_err(|_| "Peer authentication failed")?;
                if n != 0 {
                    return Err("Unexpected handshake payload".into());
                }
            }
            write = !write;
        }
        let remote_key = noise
            .get_remote_static()
            .ok_or("No authenticated peer key")?
            .to_vec();
        let verification = noise.get_handshake_hash()[..6]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<Vec<_>>()
            .join("-");
        Ok(Channel {
            stream,
            noise: noise
                .into_transport_mode()
                .map_err(|_| "Cannot establish encrypted channel")?,
            remote_key,
            verification,
        })
    })
    .await
    .map_err(|_| "Peer handshake timed out")?
}
impl Channel {
    pub async fn send<T: Serialize>(&mut self, value: &T) -> Result<()> {
        let raw = Zeroizing::new(serde_json::to_vec(value).map_err(|e| e.to_string())?);
        if raw.len() > MESSAGE {
            return Err("Sync message exceeds limit".into());
        }
        let mut bytes = Zeroizing::new((raw.len() as u32).to_be_bytes().to_vec());
        bytes.extend_from_slice(&raw);
        tokio::time::timeout(Duration::from_secs(30), async {
            let mut output = vec![0; FRAME];
            for chunk in bytes.chunks(60000) {
                let n = self
                    .noise
                    .write_message(chunk, &mut output)
                    .map_err(|_| "Encryption failed")?;
                write_frame(&mut self.stream, &output[..n]).await?;
            }
            Ok(())
        })
        .await
        .map_err(|_| "Peer send timed out")?
    }
    pub async fn receive<T: DeserializeOwned>(&mut self) -> Result<T> {
        tokio::time::timeout(Duration::from_secs(60), async {
            let first = read_frame(&mut self.stream, FRAME).await?;
            let mut plain = Zeroizing::new(vec![0; FRAME]);
            let n = self
                .noise
                .read_message(&first, &mut plain)
                .map_err(|_| "Peer message authentication failed")?;
            if n < 4 {
                return Err("Invalid message header".into());
            }
            let size =
                u32::from_be_bytes(plain[..4].try_into().map_err(|_| "Invalid message size")?)
                    as usize;
            if size > MESSAGE || size == 0 {
                return Err("Peer message exceeds limit".into());
            }
            let mut bytes = Zeroizing::new(plain[4..n].to_vec());
            while bytes.len() < size {
                let frame = read_frame(&mut self.stream, FRAME).await?;
                let n = self
                    .noise
                    .read_message(&frame, &mut plain)
                    .map_err(|_| "Peer message authentication failed")?;
                if n == 0 || bytes.len() + n > size {
                    return Err("Invalid message fragment".into());
                }
                bytes.extend_from_slice(&plain[..n]);
            }
            if bytes.len() != size {
                return Err("Invalid message length".into());
            }
            serde_json::from_slice(&bytes).map_err(|_| "Invalid authenticated message".into())
        })
        .await
        .map_err(|_| "Peer receive timed out")?
    }
}

#[cfg(test)]
mod transport_tests {
    use super::*;
    use tokio::net::TcpListener;
    const PROTOCOL: &str = "Noise_KK_25519_ChaChaPoly_SHA256";
    #[tokio::test]
    async fn authenticated_large_file_frames_roundtrip() {
        let a = snow::Builder::new(PROTOCOL.parse().unwrap())
            .generate_keypair()
            .unwrap();
        let b = snow::Builder::new(PROTOCOL.parse().unwrap())
            .generate_keypair()
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let responder = snow::Builder::new(PROTOCOL.parse().unwrap())
            .local_private_key(&b.private)
            .unwrap()
            .remote_public_key(&a.public)
            .unwrap()
            .build_responder()
            .unwrap();
        let initiator = snow::Builder::new(PROTOCOL.parse().unwrap())
            .local_private_key(&a.private)
            .unwrap()
            .remote_public_key(&b.public)
            .unwrap()
            .build_initiator()
            .unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut channel = handshake(stream, responder, false).await.unwrap();
            let value: String = channel.receive().await.unwrap();
            assert_eq!(value.len(), 14 * 1024 * 1024);
            assert!(value.bytes().all(|b| b == b'x'));
            channel
                .send(&serde_json::json!({"received":value.len()}))
                .await
                .unwrap();
        });
        let mut channel = handshake(TcpStream::connect(address).await.unwrap(), initiator, true)
            .await
            .unwrap();
        channel.send(&"x".repeat(14 * 1024 * 1024)).await.unwrap();
        let receipt: serde_json::Value = channel.receive().await.unwrap();
        assert_eq!(receipt["received"], 14 * 1024 * 1024);
        server.await.unwrap();
    }
    #[tokio::test]
    async fn wrong_pinned_peer_cannot_authenticate() {
        let a = snow::Builder::new(PROTOCOL.parse().unwrap())
            .generate_keypair()
            .unwrap();
        let b = snow::Builder::new(PROTOCOL.parse().unwrap())
            .generate_keypair()
            .unwrap();
        let wrong = snow::Builder::new(PROTOCOL.parse().unwrap())
            .generate_keypair()
            .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let responder = snow::Builder::new(PROTOCOL.parse().unwrap())
            .local_private_key(&b.private)
            .unwrap()
            .remote_public_key(&a.public)
            .unwrap()
            .build_responder()
            .unwrap();
        let initiator = snow::Builder::new(PROTOCOL.parse().unwrap())
            .local_private_key(&a.private)
            .unwrap()
            .remote_public_key(&wrong.public)
            .unwrap()
            .build_initiator()
            .unwrap();
        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            assert!(handshake(stream, responder, false).await.is_err());
        });
        assert!(
            handshake(TcpStream::connect(address).await.unwrap(), initiator, true)
                .await
                .is_err()
        );
        server.await.unwrap();
    }
}
