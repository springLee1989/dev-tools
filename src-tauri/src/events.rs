use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimEvent {
    pub ts: f64,
    pub instance_id: String,
    pub instance_name: String,
    pub level: String,
    pub kind: String,
    pub message: String,
    #[serde(default)]
    pub payload: Option<Value>,
}

impl SimEvent {
    pub fn new(
        instance_id: impl Into<String>,
        instance_name: impl Into<String>,
        level: impl Into<String>,
        kind: impl Into<String>,
        message: impl Into<String>,
        payload: Option<Value>,
    ) -> Self {
        Self {
            ts: now_secs(),
            instance_id: instance_id.into(),
            instance_name: instance_name.into(),
            level: level.into(),
            kind: kind.into(),
            message: message.into(),
            payload,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct InstanceStats {
    pub sent: u64,
    pub received: u64,
    pub errors: u64,
    pub connected: bool,
    pub last_error: String,
    pub started_at: Option<f64>,
    pub last_activity: Option<f64>,
    #[serde(default)]
    pub extra: HashMap<String, Value>,
}

pub fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

pub type EmitFn = std::sync::Arc<dyn Fn(SimEvent) + Send + Sync>;
