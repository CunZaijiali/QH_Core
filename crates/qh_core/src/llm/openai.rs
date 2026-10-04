use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::config::{AdapterConfig, SecretSource};
use crate::domain::ModelId;
use crate::domain::message::{CompletionRequest, CompletionResponse, FinishReason, Role, Usage};
use crate::error::AdapterError;
use crate::http::HttpClient;
use crate::llm::CompletionAdapter;

/// OpenAI 兼容的文本补全适配器。
/// 经配置连接 OpenAI / DeepSeek / 本地兼容服务 / 企业接口。
pub struct OpenAiCompatibleAdapter {
    model: ModelId,
    base_url: String,
    api_key: Option<String>,
    http: Arc<HttpClient>,
}

impl OpenAiCompatibleAdapter {
    pub fn from_config(cfg: &AdapterConfig, http: Arc<HttpClient>) -> Result<Self, AdapterError> {
        let base_url = cfg
            .base_url
            .clone()
            .ok_or_else(|| AdapterError::InvalidConfig("adapter.base_url is required".into()))?;
        let model = cfg
            .models
            .first()
            .cloned()
            .map(ModelId::from)
            .ok_or_else(|| AdapterError::InvalidConfig("adapter.models must not be empty".into()))?;
        let api_key = resolve_api_key(cfg.api_key_source.as_ref());
        Ok(Self {
            model,
            base_url,
            api_key,
            http,
        })
    }
}

fn resolve_api_key(source: Option<&SecretSource>) -> Option<String> {
    match source {
        Some(SecretSource::Environment(var)) => std::env::var(var).ok(),
        Some(SecretSource::File(path)) => std::fs::read_to_string(path)
            .ok()
            .map(|s| s.trim().to_string()),
        Some(SecretSource::Generated) | None => None,
    }
}

fn role_to_str(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn map_finish_reason(s: &str) -> FinishReason {
    match s {
        "stop" => FinishReason::Stop,
        "length" => FinishReason::Length,
        _ => FinishReason::Stop,
    }
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct Choice {
    message: ChoiceMessage,
    #[serde(default)]
    finish_reason: Option<String>,
}

#[derive(Deserialize)]
struct ChoiceMessage {
    #[serde(default)]
    content: String,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: Option<u64>,
    completion_tokens: Option<u64>,
    total_tokens: Option<u64>,
}

#[async_trait]
impl CompletionAdapter for OpenAiCompatibleAdapter {
    fn model(&self) -> &ModelId {
        &self.model
    }

    async fn complete(
        &self,
        request: CompletionRequest,
        cancel: CancellationToken,
    ) -> Result<CompletionResponse, AdapterError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));

        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|m| {
                serde_json::json!({
                    "role": role_to_str(&m.role),
                    "content": &m.content,
                })
            })
            .collect();

        let mut body = serde_json::json!({
            "model": request.model.0.as_str(),
            "messages": messages,
        });
        if let Some(t) = request.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(mt) = request.max_tokens {
            body["max_tokens"] = serde_json::json!(mt);
        }

        let mut builder = self.http.client().post(&url).json(&body);
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }

        let resp = tokio::select! {
            _ = cancel.cancelled() => return Err(AdapterError::Cancelled),
            r = builder.send() => r.map_err(|e| AdapterError::Request(e.to_string()))?,
        };

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(AdapterError::ProviderUnavailable(format!(
                "HTTP {status}: {text}"
            )));
        }

        let chat: ChatResponse = resp
            .json()
            .await
            .map_err(|e| AdapterError::Request(e.to_string()))?;

        let mut choices = chat.choices.into_iter();
        let first = choices.next();
        let content = first
            .as_ref()
            .map(|c| c.message.content.clone())
            .unwrap_or_default();
        let finish_reason = first
            .and_then(|c| c.finish_reason)
            .map(|s| map_finish_reason(&s))
            .unwrap_or(FinishReason::Stop);
        let usage = chat.usage.map(|u| Usage {
            prompt_tokens: u.prompt_tokens,
            completion_tokens: u.completion_tokens,
            total_tokens: u.total_tokens,
        });

        Ok(CompletionResponse {
            content,
            finish_reason,
            usage,
        })
    }
}
