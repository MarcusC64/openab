use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OABServiceManifest {
    pub api_version: String,
    pub kind: String,
    pub metadata: Metadata,
    pub spec: Spec,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Metadata {
    pub name: String,
    pub namespace: String,
    #[serde(default)]
    pub generation: u64,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec {
    #[serde(default = "default_capacity_provider")]
    pub capacity_provider: String,
    pub cpu: i32,
    pub memory: i32,
    pub task_definition: TaskDefinition,
    #[serde(default)]
    pub bootstrap_from: Option<String>,
    pub networking: Networking,
    pub config: AgentConfig,
    #[serde(default)]
    pub secrets: Vec<SecretRef>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskDefinition {
    pub image: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Networking {
    pub subnets: Vec<String>,
    pub security_groups: Vec<String>,
    #[serde(default)]
    pub assign_public_ip: bool,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretRef {
    pub name: String,
    pub value_from: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub channels: Vec<ChannelConfig>,
    #[serde(default)]
    pub backend: Option<BackendConfig>,
    #[serde(default)]
    pub steering: Option<SteeringConfig>,
    #[serde(default)]
    pub features: Option<FeaturesConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ChannelConfig {
    #[serde(rename = "type")]
    pub channel_type: String,
    #[serde(flatten)]
    pub extra: std::collections::HashMap<String, serde_yaml::Value>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct BackendConfig {
    #[serde(rename = "type")]
    pub backend_type: String,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct SteeringConfig {
    #[serde(default)]
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct FeaturesConfig {
    #[serde(default)]
    pub stt: bool,
    #[serde(default)]
    pub cronjob: bool,
}

fn default_capacity_provider() -> String {
    "FARGATE".to_string()
}

impl OABServiceManifest {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.api_version != "oab.dev/v1" {
            anyhow::bail!("unsupported apiVersion: {}", self.api_version);
        }
        if self.kind != "OABService" {
            anyhow::bail!("unsupported kind: {}", self.kind);
        }
        if self.metadata.name.is_empty() {
            anyhow::bail!("metadata.name is required");
        }
        if self.metadata.namespace.is_empty() {
            anyhow::bail!("metadata.namespace is required");
        }
        let valid_cp = ["FARGATE", "FARGATE_SPOT"];
        if !valid_cp.contains(&self.spec.capacity_provider.as_str()) {
            anyhow::bail!("capacityProvider must be FARGATE or FARGATE_SPOT");
        }
        if self.spec.networking.subnets.is_empty() {
            anyhow::bail!("networking.subnets must not be empty");
        }
        if self.spec.networking.security_groups.is_empty() {
            anyhow::bail!("networking.securityGroups must not be empty");
        }
        Ok(())
    }

    pub fn ecs_service_name(&self) -> String {
        format!("oab-{}-{}", self.metadata.namespace, self.metadata.name)
    }
}
