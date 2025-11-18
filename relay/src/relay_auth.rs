use libp2p::{
    request_response::Codec,
};
use async_trait::async_trait;
use std::io;
use futures::io::{AsyncRead, AsyncWrite, AsyncReadExt, AsyncWriteExt};

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
    #[allow(dead_code)]
    pub fn protocol_name() -> &'static str {
        "/chiral/relay-auth/1.0.0"
    }
}

// Helper function to read length-prefixed data (4-byte LE length prefix)
async fn read_framed<T: AsyncRead + Unpin + Send>(io: &mut T) -> io::Result<Vec<u8>> {
    let mut len_buf = [0u8; 4];
    io.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    
    // Prevent reading excessively large data
    if len > 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Token length {} exceeds maximum 1024 bytes", len),
        ));
    }
    
    let mut data = vec![0u8; len];
    io.read_exact(&mut data).await?;
    Ok(data)
}

// Helper function to write length-prefixed data (4-byte LE length prefix)
async fn write_framed<T: AsyncWrite + Unpin + Send>(io: &mut T, data: Vec<u8>) -> io::Result<()> {
    if data.len() > 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Token length {} exceeds maximum 1024 bytes", data.len()),
        ));
    }
    
    io.write_all(&(data.len() as u32).to_le_bytes()).await?;
    io.write_all(&data).await?;
    io.flush().await
}

#[async_trait]
impl Codec for RelayAuthCodec {
    type Protocol = RelayAuthProtocol;
    type Request = RelayAuthRequest;
    type Response = RelayAuthResponse;

    async fn read_request<T>(&mut self, _: &RelayAuthProtocol, io: &mut T) -> io::Result<Self::Request>
    where
        T: AsyncRead + Unpin + Send,
    {
        let data = read_framed(io).await?;
        Ok(RelayAuthRequest(data))
    }

    async fn read_response<T>(&mut self, _: &RelayAuthProtocol, io: &mut T) -> io::Result<Self::Response>
    where
        T: AsyncRead + Unpin + Send,
    {
        let mut buf = [0u8; 1];
        io.read_exact(&mut buf).await?;
        Ok(RelayAuthResponse(buf[0] == 1))
    }

    async fn write_request<T>(&mut self, _: &RelayAuthProtocol, io: &mut T, RelayAuthRequest(data): RelayAuthRequest) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        write_framed(io, data).await
    }

    async fn write_response<T>(&mut self, _: &RelayAuthProtocol, io: &mut T, RelayAuthResponse(accept): RelayAuthResponse) -> io::Result<()>
    where
        T: AsyncWrite + Unpin + Send,
    {
        let byte = if accept { 1u8 } else { 0u8 };
        io.write_all(&[byte]).await?;
        io.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::io::Cursor;

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

    #[tokio::test]
    async fn test_read_write_request() {
        let mut codec = RelayAuthCodec();
        let protocol = RelayAuthProtocol();
        
        // Test writing and reading a request
        let original_request = RelayAuthRequest(b"test_token_123".to_vec());
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        // Write request
        codec.write_request(&protocol, &mut cursor, original_request.clone()).await.unwrap();
        
        // Read it back
        cursor.set_position(0);
        let read_request = codec.read_request(&protocol, &mut cursor).await.unwrap();
        
        assert_eq!(original_request, read_request);
    }

    #[tokio::test]
    async fn test_read_write_response() {
        let mut codec = RelayAuthCodec();
        let protocol = RelayAuthProtocol();
        
        // Test writing and reading accept response
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        codec.write_response(&protocol, &mut cursor, RelayAuthResponse(true)).await.unwrap();
        cursor.set_position(0);
        let read_response = codec.read_response(&protocol, &mut cursor).await.unwrap();
        assert!(read_response.0);
        
        // Test writing and reading reject response
        drop(cursor);
        buffer.clear();
        let mut cursor = Cursor::new(&mut buffer);
        codec.write_response(&protocol, &mut cursor, RelayAuthResponse(false)).await.unwrap();
        cursor.set_position(0);
        let read_response = codec.read_response(&protocol, &mut cursor).await.unwrap();
        assert!(!read_response.0);
    }

    #[tokio::test]
    async fn test_token_length_limit() {
        let mut codec = RelayAuthCodec();
        let protocol = RelayAuthProtocol();
        
        // Test that tokens over 1024 bytes are rejected
        let large_token = vec![0u8; 1025];
        let mut buffer = Vec::new();
        let mut cursor = Cursor::new(&mut buffer);
        
        let result = codec.write_request(&protocol, &mut cursor, RelayAuthRequest(large_token)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exceeds maximum"));
    }
}