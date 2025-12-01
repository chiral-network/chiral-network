use async_trait::async_trait;
use futures::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use libp2p::request_response::{self as rr, ProtocolName};
use serde::{Deserialize, Serialize};
use std::io;

// internal imports
use crate::encryption::EncryptedAesKeyBundle;

// =========================================================================
// Helper: Framing Logic (4-byte LE Length Prefix)
// =========================================================================

async fn read_framed<T>(io: &mut T) -> io::Result<Vec<u8>>
where
    T: AsyncRead + Unpin + Send,
{
    let mut len_buf = [0u8; 4];
    io.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;

    // Safety check: prevent massive allocations from malicious peers
    if len > 10 * 1024 * 1024 {
        // 10MB limit example
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Message too large",
        ));
    }

    let mut data = vec![0u8; len];
    io.read_exact(&mut data).await?;
    Ok(data)
}

async fn write_framed<T>(io: &mut T, data: Vec<u8>) -> io::Result<()>
where
    T: AsyncWrite + Unpin + Send,
{
    io.write_all(&(data.len() as u32).to_le_bytes()).await?;
    io.write_all(&data).await?;
    io.flush().await
}

// =========================================================================
// 1. Key Request Protocol
// =========================================================================

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyRequestProtocol;

impl ProtocolName for KeyRequestProtocol {
    fn protocol_name(&self) -> &[u8] {
        b"/chiral/key-request/1.0.0"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyRequest {
    pub merkle_root: String,
    #[serde(with = "serde_bytes")]
    pub recipient_public_key: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyResponse {
    pub encrypted_bundle: Option<EncryptedAesKeyBundle>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct KeyRequestCodec;

#[async_trait]
impl rr::Codec for KeyRequestCodec {
    type Protocol = KeyRequestProtocol;
    type Request = KeyRequest;
    type Response = KeyResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data =
            serde_json::to_vec(&req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data =
            serde_json::to_vec(&res).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }
}

// =========================================================================
// 2. Proxy / Echo Protocol
// =========================================================================

#[derive(Debug, Clone)]
pub struct EchoRequest(pub Vec<u8>);

#[derive(Debug, Clone)]
pub struct EchoResponse(pub Vec<u8>);

#[derive(Clone, Debug, Default)]
pub struct ProxyCodec;

#[async_trait]
impl rr::Codec for ProxyCodec {
    type Protocol = String; // Using String allows dynamic protocol names
    type Request = EchoRequest;
    type Response = EchoResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        Ok(EchoRequest(read_framed(io).await?))
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        Ok(EchoResponse(read_framed(io).await?))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        EchoRequest(data): Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_framed(io, data).await
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        EchoResponse(data): Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_framed(io, data).await
    }
}

// =========================================================================
// 3. WebRTC Signaling Protocol
// =========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCOfferRequest {
    pub offer_sdp: String,
    pub file_hash: String,
    pub requester_peer_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebRTCAnswerResponse {
    pub answer_sdp: String,
}

#[derive(Clone, Debug, Default)]
pub struct WebRTCSignalingCodec;

#[async_trait]
impl rr::Codec for WebRTCSignalingCodec {
    type Protocol = String;
    type Request = WebRTCOfferRequest;
    type Response = WebRTCAnswerResponse;

    async fn read_request<T>(&mut self, _: &Self::Protocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn read_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
    ) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        serde_json::from_slice(&data).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
    }

    async fn write_request<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        req: Self::Request,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data =
            serde_json::to_vec(&req).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }

    async fn write_response<T>(
        &mut self,
        _: &Self::Protocol,
        io: &mut T,
        res: Self::Response,
    ) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let data =
            serde_json::to_vec(&res).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        write_framed(io, data).await
    }
}
