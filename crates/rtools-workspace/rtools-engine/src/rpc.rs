use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const MAX_FRAME_SIZE: u32 = 16 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub protocol_version: u32,
    pub message_type: String,
    pub request_id: String,
    pub method: String,
    #[serde(default)]
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub protocol_version: u32,
    pub message_type: String,
    pub request_id: String,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub protocol_version: u32,
    pub message_type: String,
    pub event: String,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
}

pub async fn read_frame<R: AsyncReadExt + Unpin>(reader: &mut R) -> Result<Vec<u8>> {
    let len = reader.read_u32().await?;
    if len == 0 || len > MAX_FRAME_SIZE {
        anyhow::bail!("invalid frame size: {len}");
    }
    let mut buf = vec![0; len as usize];
    reader.read_exact(&mut buf).await?;
    Ok(buf)
}

pub async fn write_frame<W: AsyncWriteExt + Unpin>(writer: &mut W, data: &[u8]) -> Result<()> {
    if data.is_empty() || data.len() > MAX_FRAME_SIZE as usize {
        anyhow::bail!("invalid frame size: {}", data.len());
    }
    writer.write_u32(data.len() as u32).await?;
    writer.write_all(data).await?;
    writer.flush().await?;
    Ok(())
}

pub fn dispatch(method: &str) -> std::result::Result<Value, RpcError> {
    match method {
        "system.health" => Ok(serde_json::json!({
            "ok": true,
            "app": "rtools",
            "protocol_version": crate::PROTOCOL_VERSION
        })),
        _ => Err(RpcError {
            code: "METHOD_NOT_FOUND".into(),
            message: format!("unknown method: {method}"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn frames_roundtrip() {
        let (mut a, mut b) = tokio::io::duplex(1024);
        write_frame(&mut a, b"hello").await.unwrap();
        assert_eq!(read_frame(&mut b).await.unwrap(), b"hello");
    }
}
