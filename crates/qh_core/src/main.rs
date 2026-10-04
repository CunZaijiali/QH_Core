use qh_core::config::{AdapterConfig, AdapterProvider, CoreConfig, SecretSource};
use qh_core::core::AgentCore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = demo_config();
    let core = AgentCore::bootstrap(config).await?;

    // 演示一次补全（仅当配置了 adapter 时）
    match core.complete("用一句话介绍你自己").await {
        Ok(resp) => println!("assistant: {}", resp.content),
        Err(e) => tracing::warn!("skip completion: {e}"),
    }

    core.run().await?;
    Ok(())
}

/// 演示配置：设置了 DEEPSEEK_API_KEY 环境变量才启用 adapter。
fn demo_config() -> CoreConfig {
    let mut config = CoreConfig::default();
    if std::env::var("DEEPSEEK_API_KEY").is_ok() {
        config.adapters.push(AdapterConfig {
            id: "deepseek".into(),
            provider: AdapterProvider::OpenAiCompatible,
            enabled: true,
            models: vec!["deepseek-chat".into()],
            base_url: Some("https://api.deepseek.com".into()),
            api_key_source: Some(SecretSource::Environment("DEEPSEEK_API_KEY".into())),
            ..AdapterConfig::default()
        });
    }
    config
}
