use libp2p::{
    request_response::{
        Behaviour, Config, Event, ProtocolSupport, Message, Codec,
    },
};
use async_trait::async_trait;
use std::io;

#[derive(Clone)]
pub struct RelayAuthProtocol();

#[derive(Clone, Default)]
pub struct RelayAuthCodec();

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayAuthRequest(pub Vec<u8>); // the token bytes

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayAuthResponse(pub bool); // accepted or not

impl AsRef<str> for RelayAuthProtocol {
    fn as_ref(&self) -> &str {
        "/chiral/relay-auth/1.0.0"
    }
}

impl RelayAuthProtocol {
    pub fn protocol_name() -> &'static str {
        "/chiral/relay-auth/1.0.0"
    }
}

#[async_trait]
impl Codec for RelayAuthCodec {
    type Protocol = RelayAuthProtocol;
    type Request = RelayAuthRequest;
    type Response = RelayAuthResponse;

    async fn read_request<T>(&mut self, _: &RelayAuthProtocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: async_std::io::Read + Unpin + Send,
    {
        let mut buf = vec![0u8; 128];
        let n = async_std::io::ReadExt::read(io, &mut buf).await?;
        buf.truncate(n);
        Ok(RelayAuthRequest(buf))
    }

    async fn read_response<T>(&mut self, _: &RelayAuthProtocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: async_std::io::Read + Unpin + Send,
    {
        let mut buf = [0u8; 1];
        async_std::io::ReadExt::read_exact(io, &mut buf).await?;
        Ok(RelayAuthResponse(buf[0] == 1))
    }

    async fn write_request<T>(&mut self, _: &RelayAuthProtocol, io: &mut T, RelayAuthRequest(data): RelayAuthRequest) -> io::Result<()>
    where
        T: async_std::io::Write + Unpin + Send,
    {
        async_std::io::WriteExt::write_all(io, &data).await
    }

    async fn write_response<T>(&mut self, _: &RelayAuthProtocol, io: &mut T, RelayAuthResponse(accept): RelayAuthResponse) -> io::Result<()>
    where
        T: async_std::io::Write + Unpin + Send,
    {
        let byte = if accept { 1u8 } else { 0u8 };
        async_std::io::WriteExt::write_all(io, &[byte]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_protocol_name() {
        let protocol = RelayAuthProtocol();
        assert_eq!(protocol.as_ref(), "/chiral/relay-auth/1.0.0");
        assert_eq!(RelayAuthProtocol::protocol_name(), "/chiral/relay-auth/1.0.0");
    }

    #[test]
    fn test_request_response_creation() {
        let request = RelayAuthRequest(b"test_token".to_vec());
        assert_eq!(request.0, b"test_token");

        let response_accept = RelayAuthResponse(true);
        assert!(response_accept.0);

        let response_reject = RelayAuthResponse(false);
        assert!(!response_reject.0);
    }

    #[test]
    fn test_request_cloning() {
        let request = RelayAuthRequest(b"clone_test".to_vec());
        let cloned = request.clone();
        assert_eq!(request, cloned);
    }
}