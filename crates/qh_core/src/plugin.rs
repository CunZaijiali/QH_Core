mod manager;
mod manifest;
mod protocol;
mod sidecar;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};

use crate::error::PluginError;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginManifest {
    pub plugin: PluginMetadata,
    #[serde(default, rename = "function")]
    pub functions: Vec<ExposedFunction>,
    #[serde(default)]
    pub permissions: Vec<PermissionDefinition>,
    #[serde(default)]
    pub subscriptions: Vec<EventSubscription>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

/// 插件分类：按实现方式区分。
///
/// - `Native`：Rust 原生插件，进程内直接实现 Core 的 trait。
/// - `Sidecar`：其他语言插件，独立进程经长度前缀 JSON 协议通信。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum PluginKind {
    /// Rust 原生插件：进程内，直接实现 Core 的 trait。
    Native,
    /// 其他语言插件：独立进程。
    Sidecar {
        /// 运行时标识，如 `python` / `node` / `go` / `rust`。
        runtime: String,
    },
}

impl Default for PluginKind {
    fn default() -> Self {
        Self::Native
    }
}

/// 插件生命周期状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginState {
    /// 已发现，尚未加载（懒加载等待中）。
    Discovered,
    /// 加载中。
    Loading,
    /// 握手中（仅 Sidecar）。
    Handshaking,
    /// 就绪，可调用。
    Ready,
    /// 正在停止。
    Stopping,
    /// 已停止。
    Stopped,
    /// 失败（崩溃或初始化失败）。
    Failed,
}

impl PluginState {
    /// 是否已就绪、可调用。
    pub fn is_ready(self) -> bool {
        matches!(self, Self::Ready)
    }

    /// 是否已进入终态。
    pub fn is_settled(self) -> bool {
        matches!(self, Self::Ready | Self::Stopped | Self::Failed)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entrypoint: PathBuf,
    /// 插件分类，缺省视为 Rust 原生插件。
    #[serde(default)]
    pub kind: PluginKind,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub protocol_version: Option<String>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ExposedFunction {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub fields: BTreeMap<String, FunctionField>,
    #[serde(default)]
    pub returns: Option<toml::Value>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionField {
    #[serde(rename = "type")]
    pub field_type: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub default: Option<toml::Value>,
    #[serde(default)]
    pub enum_values: Vec<toml::Value>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PermissionDefinition {
    pub resource: String,
    #[serde(default)]
    pub actions: Vec<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EventSubscription {
    pub event: String,
    #[serde(default)]
    pub handler: Option<String>,
    #[serde(default)]
    pub filter: BTreeMap<String, toml::Value>,
    #[serde(default, flatten)]
    pub extensions: BTreeMap<String, toml::Value>,
}

impl PluginManifest {
    pub fn from_toml(source: &str) -> Result<Self, PluginError> {
        let manifest: Self = toml::from_str(source)?;
        manifest.validate()?;
        Ok(manifest)
    }

    pub fn load(path: impl AsRef<Path>) -> Result<Self, PluginError> {
        Self::from_toml(&fs::read_to_string(path)?)
    }

    pub fn validate(&self) -> Result<(), PluginError> {
        if self.plugin.id.trim().is_empty()
            || self.plugin.name.trim().is_empty()
            || self.plugin.version.trim().is_empty()
        {
            return Err(PluginError::InvalidManifest(
                "plugin id, name and version must not be empty".into(),
            ));
        }
        if let PluginKind::Sidecar { runtime } = &self.plugin.kind {
            if runtime.trim().is_empty() {
                return Err(PluginError::InvalidManifest(
                    "sidecar plugin must declare a runtime".into(),
                ));
            }
        }
        let mut names = std::collections::HashSet::new();
        for function in &self.functions {
            if function.name.trim().is_empty() {
                return Err(PluginError::InvalidManifest(
                    "function name must not be empty".into(),
                ));
            }
            if !names.insert(&function.name) {
                return Err(PluginError::InvalidManifest(format!(
                    "duplicate function: {}",
                    function.name
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Default)]
pub struct PluginManager {
    manifests: HashMap<String, PluginManifest>,
    states: HashMap<String, PluginState>,
}

impl PluginManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(
        &mut self,
        manifest: PluginManifest,
    ) -> Result<Option<PluginManifest>, PluginError> {
        manifest.validate()?;
        let id = manifest.plugin.id.clone();
        self.states.insert(id.clone(), PluginState::Discovered);
        Ok(self.manifests.insert(id, manifest))
    }

    pub fn load_file(&mut self, path: impl AsRef<Path>) -> Result<&PluginManifest, PluginError> {
        let manifest = PluginManifest::load(path)?;
        let id = manifest.plugin.id.clone();
        self.register(manifest)?;
        Ok(&self.manifests[&id])
    }

    pub fn load_dir(&mut self, path: impl AsRef<Path>) -> Result<Vec<String>, PluginError> {
        let mut paths = fs::read_dir(path)?.collect::<Result<Vec<_>, _>>()?;
        paths.sort_by_key(|entry| entry.path());
        let mut loaded = Vec::new();
        for entry in paths {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) == Some("toml") {
                loaded.push(self.load_file(path)?.plugin.id.clone());
            }
        }
        Ok(loaded)
    }

    pub fn get(&self, id: &str) -> Option<&PluginManifest> {
        self.manifests.get(id)
    }

    /// 查询插件生命周期状态。
    pub fn state(&self, id: &str) -> Option<PluginState> {
        self.states.get(id).copied()
    }

    /// 状态转换为「加载中」。
    pub fn mark_loading(&mut self, id: &str) -> Result<(), PluginError> {
        self.transition(id, PluginState::Loading)
    }

    /// 状态转换为「握手中」。
    pub fn mark_handshaking(&mut self, id: &str) -> Result<(), PluginError> {
        self.transition(id, PluginState::Handshaking)
    }

    /// 状态转换为「就绪」。
    pub fn mark_ready(&mut self, id: &str) -> Result<(), PluginError> {
        self.transition(id, PluginState::Ready)
    }

    /// 状态转换为「失败」。
    pub fn mark_failed(&mut self, id: &str) -> Result<(), PluginError> {
        self.transition(id, PluginState::Failed)
    }

    /// 状态转换为「已停止」。
    pub fn mark_stopped(&mut self, id: &str) -> Result<(), PluginError> {
        self.transition(id, PluginState::Stopped)
    }

    /// 处于「已发现」状态、等待懒加载的插件 id。
    pub fn pending(&self) -> Vec<&str> {
        self.states
            .iter()
            .filter(|(_, state)| matches!(state, PluginState::Discovered))
            .map(|(id, _)| id.as_str())
            .collect()
    }

    fn transition(&mut self, id: &str, next: PluginState) -> Result<(), PluginError> {
        match self.states.get_mut(id) {
            Some(state) => {
                *state = next;
                Ok(())
            }
            None => Err(PluginError::InvalidManifest(format!(
                "unknown plugin: {id}"
            ))),
        }
    }

    /// 该插件是否为 Rust 原生插件。
    pub fn is_native(&self, id: &str) -> bool {
        self.manifests
            .get(id)
            .is_some_and(|m| matches!(m.plugin.kind, PluginKind::Native))
    }

    /// 按分类列出：Rust 原生插件。
    pub fn native_plugins(&self) -> Vec<&PluginManifest> {
        self.manifests
            .values()
            .filter(|m| matches!(m.plugin.kind, PluginKind::Native))
            .collect()
    }

    /// 按分类列出：其他语言（独立进程）插件。
    pub fn sidecar_plugins(&self) -> Vec<&PluginManifest> {
        self.manifests
            .values()
            .filter(|m| matches!(m.plugin.kind, PluginKind::Sidecar { .. }))
            .collect()
    }
    pub fn iter(&self) -> impl Iterator<Item = &PluginManifest> {
        self.manifests.values()
    }
    pub fn remove(&mut self, id: &str) -> Option<PluginManifest> {
        self.states.remove(id);
        self.manifests.remove(id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NATIVE: &str = r#"
        [plugin]
        id = "example.native"
        name = "Native"
        version = "0.1.0"
        entrypoint = "plugins/native_plugin"
        [plugin.kind]
        type = "native"
    "#;

    const SIDECAR: &str = r#"
        [plugin]
        id = "example.python"
        name = "Python"
        version = "0.1.0"
        entrypoint = "plugins/echo.py"
        [plugin.kind]
        type = "sidecar"
        runtime = "python"
    "#;

    #[test]
    fn classifies_native_and_sidecar() {
        let mut manager = PluginManager::new();
        manager
            .register(PluginManifest::from_toml(NATIVE).unwrap())
            .unwrap();
        manager
            .register(PluginManifest::from_toml(SIDECAR).unwrap())
            .unwrap();

        assert_eq!(manager.native_plugins().len(), 1);
        assert_eq!(manager.sidecar_plugins().len(), 1);
        assert!(manager.is_native("example.native"));
        assert!(!manager.is_native("example.python"));
    }

    #[test]
    fn defaults_to_native_when_kind_missing() {
        let manifest = PluginManifest::from_toml(
            r#"
            [plugin]
            id = "example.default"
            name = "Default"
            version = "0.1.0"
            entrypoint = "plugins/x"
        "#,
        )
        .unwrap();

        assert_eq!(manifest.plugin.kind, PluginKind::Native);
    }

    #[test]
    fn lifecycle_transitions() {
        let mut manager = PluginManager::new();
        manager
            .register(PluginManifest::from_toml(NATIVE).unwrap())
            .unwrap();

        assert_eq!(
            manager.state("example.native"),
            Some(PluginState::Discovered)
        );
        assert_eq!(manager.pending(), vec!["example.native"]);

        manager.mark_loading("example.native").unwrap();
        assert_eq!(manager.state("example.native"), Some(PluginState::Loading));

        manager.mark_ready("example.native").unwrap();
        assert!(manager.state("example.native").unwrap().is_ready());
        assert!(manager.pending().is_empty());

        assert!(manager.mark_failed("unknown-plugin").is_err());
    }

    #[test]
    fn rejects_sidecar_without_runtime() {
        let result = PluginManifest::from_toml(
            r#"
            [plugin]
            id = "example.bad"
            name = "Bad"
            version = "0.1.0"
            entrypoint = "plugins/x"
            [plugin.kind]
            type = "sidecar"
            runtime = ""
        "#,
        );

        assert!(result.is_err());
    }
}
