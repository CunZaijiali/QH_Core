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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub entrypoint: PathBuf,
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
        Ok(self.manifests.insert(manifest.plugin.id.clone(), manifest))
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
    pub fn iter(&self) -> impl Iterator<Item = &PluginManifest> {
        self.manifests.values()
    }
    pub fn remove(&mut self, id: &str) -> Option<PluginManifest> {
        self.manifests.remove(id)
    }
}
