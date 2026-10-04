use async_trait::async_trait;
use tokio_util::sync::CancellationToken;

use crate::domain::ModelId;
use crate::{
    domain::message::{CompletionRequest, CompletionResponse},
    error::AdapterError,
};

/// Core 最小适配器契约：一个默认模型 + `complete`。
/// 流式、多 Provider、fallback、middleware 等不进 Core，由扩展层承担。
#[async_trait]
pub trait CompletionAdapter: Send + Sync {
    fn model(&self) -> &ModelId;

    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, AdapterError>;
}
