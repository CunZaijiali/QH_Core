use qh_core::config::{AdapterConfig, AdapterProvider, CoreConfig, SecretSource};
use qh_core::core::AgentCore;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let config = demo_config();
    let core = AgentCore::bootstrap(config).await?;

    // 演示多轮会话（仅当配置了 adapter 时）
    if core.has_adapter() {
        let id = core.create_session().await?;
        println!("== session {id} ==");

        for prompt in ["你好，我叫小明。", "我叫什么名字？"] {
            match core.send_message(id, prompt).await {
                Ok(reply) => println!("user: {prompt}\nassistant: {reply}\n"),
                Err(err) => tracing::warn!("send_message failed: {err}"),
            }
        }

        let history = core.history(id).await?;
        println!("history has {} messages", history.len());
        core.delete_session(id).await?;
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
