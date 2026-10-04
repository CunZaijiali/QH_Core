use serde::{Deserialize, Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub const DEFAULT_MAX_FRAME_SIZE: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum IpcMessage {
    Request {
        id: String,
        method: String,
        payload: serde_json::Value,
    },
    Response {
        id: String,
        result: serde_json::Value,
    },
    Error {
        id: Option<String>,
        code: i64,
        message: String,
        data: Option<serde_json::Value>,
    },
    Event {
        topic: String,
        payload: serde_json::Value,
    },
    Ping {
        nonce: u64,
    },
    Pong {
        nonce: u64,
    },
}

/// Length-prefixed JSON transport usable with TCP, Unix sockets, named pipes, or duplex streams.
pub struct IpcConnection<T> {
    io: T,
    max_frame_size: usize,
}

impl<T> IpcConnection<T> {
    pub fn new(io: T) -> Self {
        Self {
            io,
            max_frame_size: DEFAULT_MAX_FRAME_SIZE,
        }
    }
    pub fn with_max_frame_size(io: T, max_frame_size: usize) -> Self {
        Self { io, max_frame_size }
    }
    pub fn into_inner(self) -> T {
        self.io
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin> IpcConnection<T> {
    pub async fn send(&mut self, message: &IpcMessage) -> Result<(), IpcError> {
        self.send_value(message).await
    }

    pub async fn receive(&mut self) -> Result<IpcMessage, IpcError> {
        self.receive_value().await
    }

    pub async fn send_value<S: Serialize + ?Sized>(&mut self, value: &S) -> Result<(), IpcError> {
        let payload = serde_json::to_vec(value)?;
        if payload.len() > self.max_frame_size || payload.len() > u32::MAX as usize {
            return Err(IpcError::FrameTooLarge(payload.len()));
        }
        self.io.write_u32(payload.len() as u32).await?;
        self.io.write_all(&payload).await?;
        self.io.flush().await?;
        Ok(())
    }

    pub async fn receive_value<D: DeserializeOwned>(&mut self) -> Result<D, IpcError> {
        let length = self.io.read_u32().await? as usize;
        if length > self.max_frame_size {
            return Err(IpcError::FrameTooLarge(length));
        }
        let mut payload = vec![0; length];
        self.io.read_exact(&mut payload).await?;
        Ok(serde_json::from_slice(&payload)?)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum IpcError {
    #[error("IPC I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid IPC JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("IPC frame is too large: {0} bytes")]
    FrameTooLarge(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn exchanges_framed_messages() {
        let (left, right) = tokio::io::duplex(1024);
        let mut sender = IpcConnection::new(left);
        let mut receiver = IpcConnection::new(right);
        let message = IpcMessage::Ping { nonce: 42 };
        let (sent, received) = tokio::join!(sender.send(&message), receiver.receive());
        sent.unwrap();
        assert_eq!(received.unwrap(), message);
    }
}
