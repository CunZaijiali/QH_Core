use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

/// A granted operation. Unknown operations can be represented without changing the model.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Read,
    Write,
    Execute,
    Invoke,
    Delegate,
    Custom(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResourcePattern {
    File {
        path: String,
        max_size: Option<u64>,
    },
    Network {
        host: String,
        #[serde(default)]
        ports: Vec<u16>,
        protocol: Option<String>,
    },
    Tool {
        name: String,
        max_calls: Option<u32>,
    },
    Extension {
        point: String,
    },
    Process {
        command: String,
        max_memory: Option<u64>,
    },
    Plugin {
        target: String,
        #[serde(default)]
        actions: Vec<Action>,
    },
    Custom {
        kind: String,
        value: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Constraint {
    Session { id: String },
    TimeWindow { from: u64, to: u64 },
    RateLimit { max: u32, window_seconds: u64 },
    RequiresUserConsent,
    AuditLevel { level: String },
    ReadOnly,
    NoCredentials,
    Custom { name: String, value: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SensitivityLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    pub subject: String,
    pub effect: Effect,
    pub actions: HashSet<Action>,
    pub resource: ResourcePattern,
    #[serde(default)]
    pub constraints: Vec<Constraint>,
    pub valid_from: Option<u64>,
    pub valid_until: Option<u64>,
    #[serde(default)]
    pub delegatable: bool,
    pub sensitivity: SensitivityLevel,
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Allow,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapabilityRequest {
    pub subject: String,
    pub action: Action,
    pub resource: ResourcePattern,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EvaluationContext {
    pub session_id: Option<String>,
    pub user_consent: bool,
    pub audit_level: Option<String>,
    pub includes_credentials: bool,
    pub calls_in_window: Option<u32>,
}

impl CapabilityRequest {
    pub fn now(subject: impl Into<String>, action: Action, resource: ResourcePattern) -> Self {
        Self {
            subject: subject.into(),
            action,
            resource,
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
    NotGranted,
}

/// Evaluates all matching capabilities as a union, with an explicit deny taking precedence.
pub fn evaluate(capabilities: &[Capability], request: &CapabilityRequest) -> Decision {
    evaluate_with_context(capabilities, request, &EvaluationContext::default())
}

pub fn evaluate_with_context(
    capabilities: &[Capability],
    request: &CapabilityRequest,
    context: &EvaluationContext,
) -> Decision {
    let mut allowed = false;
    for capability in capabilities {
        if !capability.matches_with_context(request, context) {
            continue;
        }
        match capability.effect {
            Effect::Deny => return Decision::Deny,
            Effect::Allow => allowed = true,
        }
    }
    if allowed {
        Decision::Allow
    } else {
        Decision::NotGranted
    }
}

impl Capability {
    pub fn matches(&self, request: &CapabilityRequest) -> bool {
        self.matches_with_context(request, &EvaluationContext::default())
    }

    pub fn matches_with_context(
        &self,
        request: &CapabilityRequest,
        context: &EvaluationContext,
    ) -> bool {
        self.subject == request.subject
            && self.actions.contains(&request.action)
            && self.valid_from.is_none_or(|from| request.timestamp >= from)
            && self
                .valid_until
                .is_none_or(|until| request.timestamp <= until)
            && self.resource.matches(&request.resource)
            && self.constraints.iter().all(|constraint| match constraint {
                Constraint::TimeWindow { from, to } => {
                    request.timestamp >= *from && request.timestamp <= *to
                }
                Constraint::Session { id } => context.session_id.as_ref() == Some(id),
                Constraint::RateLimit { max, .. } => {
                    context.calls_in_window.is_some_and(|calls| calls < *max)
                }
                Constraint::RequiresUserConsent => context.user_consent,
                Constraint::AuditLevel { level } => context.audit_level.as_ref() == Some(level),
                Constraint::ReadOnly => request.action == Action::Read,
                Constraint::NoCredentials => !context.includes_credentials,
                Constraint::Custom { .. } => false,
            })
    }

    /// Creates a narrower delegated capability. The caller supplies the narrowed resource.
    pub fn delegate(
        &self,
        id: impl Into<String>,
        subject: impl Into<String>,
        actions: HashSet<Action>,
        resource: ResourcePattern,
        valid_until: Option<u64>,
    ) -> Result<Self, CapabilityError> {
        if !self.delegatable {
            return Err(CapabilityError::NotDelegatable);
        }
        if !actions.is_subset(&self.actions) || !self.resource.contains(&resource) {
            return Err(CapabilityError::Escalation);
        }
        if self
            .valid_until
            .is_some_and(|parent| valid_until.is_none_or(|child| child > parent))
        {
            return Err(CapabilityError::Escalation);
        }
        Ok(Self {
            id: id.into(),
            subject: subject.into(),
            effect: self.effect,
            actions,
            resource,
            constraints: self.constraints.clone(),
            valid_from: self.valid_from,
            valid_until,
            delegatable: false,
            sensitivity: self.sensitivity,
            signature: None,
        })
    }
}

impl ResourcePattern {
    fn matches(&self, requested: &Self) -> bool {
        self.contains(requested)
    }

    fn contains(&self, requested: &Self) -> bool {
        match (self, requested) {
            (
                Self::File { path, max_size },
                Self::File {
                    path: other,
                    max_size: size,
                },
            ) => wildcard_match(path, other) && limit_contains(*max_size, *size),
            (
                Self::Network {
                    host,
                    ports,
                    protocol,
                },
                Self::Network {
                    host: other,
                    ports: requested_ports,
                    protocol: requested_protocol,
                },
            ) => {
                wildcard_match(host, other)
                    && (ports.is_empty() || requested_ports.iter().all(|port| ports.contains(port)))
                    && (protocol.is_none() || protocol == requested_protocol)
            }
            (
                Self::Tool { name, max_calls },
                Self::Tool {
                    name: other,
                    max_calls: calls,
                },
            ) => {
                wildcard_match(name, other)
                    && limit_contains((*max_calls).map(u64::from), (*calls).map(u64::from))
            }
            (Self::Extension { point }, Self::Extension { point: other }) => {
                wildcard_match(point, other)
            }
            (
                Self::Process {
                    command,
                    max_memory,
                },
                Self::Process {
                    command: other,
                    max_memory: memory,
                },
            ) => wildcard_match(command, other) && limit_contains(*max_memory, *memory),
            (
                Self::Plugin { target, actions },
                Self::Plugin {
                    target: other,
                    actions: requested_actions,
                },
            ) => {
                wildcard_match(target, other)
                    && (actions.is_empty()
                        || requested_actions
                            .iter()
                            .all(|action| actions.contains(action)))
            }
            (
                Self::Custom { kind, value },
                Self::Custom {
                    kind: other_kind,
                    value: other_value,
                },
            ) => kind == other_kind && wildcard_match(value, other_value),
            _ => false,
        }
    }
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    pattern == "*"
        || pattern == value
        || pattern
            .strip_suffix('*')
            .is_some_and(|prefix| value.starts_with(prefix))
}

fn limit_contains(granted: Option<u64>, requested: Option<u64>) -> bool {
    match (granted, requested) {
        (Some(granted), Some(requested)) => requested <= granted,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CapabilityError {
    #[error("capability cannot be delegated")]
    NotDelegatable,
    #[error("delegation would expand the parent capability")]
    Escalation,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capability(effect: Effect, path: &str) -> Capability {
        Capability {
            id: "cap".into(),
            subject: "plugin:a".into(),
            effect,
            actions: HashSet::from([Action::Read]),
            resource: ResourcePattern::File {
                path: path.into(),
                max_size: None,
            },
            constraints: vec![],
            valid_from: None,
            valid_until: None,
            delegatable: true,
            sensitivity: SensitivityLevel::Low,
            signature: None,
        }
    }

    #[test]
    fn explicit_deny_wins() {
        let request = CapabilityRequest {
            subject: "plugin:a".into(),
            action: Action::Read,
            resource: ResourcePattern::File {
                path: "/tmp/private/a".into(),
                max_size: Some(1),
            },
            timestamp: 1,
        };
        assert_eq!(
            evaluate(
                &[
                    capability(Effect::Allow, "/tmp/*"),
                    capability(Effect::Deny, "/tmp/private/*")
                ],
                &request
            ),
            Decision::Deny
        );
    }
}
