use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
pub struct GetChildStatusRequest {
    #[serde(default)]
    pub include_completed: bool,
}

#[derive(Debug, Deserialize)]
pub struct SpawnControlRequest {
    pub child_context_id: FlexibleI64,
    pub action: SpawnControlDirective,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpawnControlDirective {
    Stop,
    Restart,
    UpdateParams,
}

#[derive(Debug, Deserialize)]
pub struct GetCompletionSummaryRequest {
    #[serde(default)]
    pub child_context_id: Option<FlexibleI64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TriggerMode {
    Spawn,
    Fork,
}

fn default_trigger_mode() -> TriggerMode {
    TriggerMode::Spawn
}

#[derive(Debug, Deserialize)]
pub struct TriggerContextRequest {
    #[serde(default)]
    pub target_context_id: Option<FlexibleI64>,
    pub agent_name: String,
    #[serde(default = "default_trigger_mode")]
    pub mode: TriggerMode,
    #[serde(default)]
    pub instructions: Option<String>,
    #[serde(default, alias = "message")]
    pub message: Option<String>,
    #[serde(default = "default_execute")]
    pub execute: bool,
}

impl TriggerContextRequest {
    pub fn instruction_text(&self) -> Option<&str> {
        self.instructions
            .as_deref()
            .or(self.message.as_deref())
            .map(str::trim)
            .filter(|value| !value.is_empty())
    }

    pub fn normalized_agent_name(&self) -> String {
        self.agent_name.trim().to_string()
    }

    pub fn is_fork_mode(&self) -> bool {
        matches!(self.mode, TriggerMode::Fork)
    }
}

fn default_execute() -> bool {
    true
}

#[derive(Debug, Clone, Copy)]
pub struct FlexibleI64(pub i64);

impl<'de> Deserialize<'de> for FlexibleI64 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct FlexibleI64Visitor;

        impl serde::de::Visitor<'_> for FlexibleI64Visitor {
            type Value = FlexibleI64;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("an i64 or string-encoded i64")
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                Ok(FlexibleI64(value))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                i64::try_from(value)
                    .map(FlexibleI64)
                    .map_err(|_| E::custom("u64 value overflows i64"))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                value.parse::<i64>().map(FlexibleI64).map_err(|error| {
                    E::custom(format!("invalid i64 string '{}': {}", value, error))
                })
            }
        }

        deserializer.deserialize_any(FlexibleI64Visitor)
    }
}
