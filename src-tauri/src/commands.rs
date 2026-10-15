use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use schemars::schema_for;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::engine::SharedEngine;
use crate::events::InstanceStats;
use crate::models::{InstanceType, Profile, SimInstance};

#[derive(Clone, Serialize)]
pub struct AppStateView {
    pub profile: Profile,
    pub profile_path: Option<String>,
    pub running: HashMap<String, bool>,
    pub stats: HashMap<String, InstanceStats>,
}

fn view(engine: &SharedEngine) -> AppStateView {
    AppStateView {
        profile: engine.profile(),
        profile_path: engine
            .profile_path()
            .map(|p| p.to_string_lossy().into_owned()),
        running: engine.running_map(),
        stats: engine.all_stats(),
    }
}

fn map_err(err: impl std::fmt::Display) -> String {
    err.to_string()
}

#[tauri::command]
pub fn get_state(engine: State<'_, SharedEngine>) -> AppStateView {
    view(&engine)
}

#[tauri::command]
pub fn load_profile(engine: State<'_, SharedEngine>, path: String) -> Result<AppStateView, String> {
    engine.load_profile(path).map_err(map_err)?;
    Ok(view(&engine))
}

#[tauri::command]
pub fn load_profile_json(
    engine: State<'_, SharedEngine>,
    data: String,
) -> Result<AppStateView, String> {
    engine.load_profile_data(&data).map_err(map_err)?;
    Ok(view(&engine))
}

#[tauri::command]
pub fn save_profile(
    engine: State<'_, SharedEngine>,
    path: Option<String>,
) -> Result<AppStateView, String> {
    engine
        .save_profile(path.map(PathBuf::from))
        .map_err(map_err)?;
    Ok(view(&engine))
}

#[tauri::command]
pub fn dump_profile(engine: State<'_, SharedEngine>) -> Result<String, String> {
    engine.dump_profile().map_err(map_err)
}

#[tauri::command]
pub fn set_profile_meta(
    engine: State<'_, SharedEngine>,
    name: String,
    description: String,
) -> Result<AppStateView, String> {
    engine.set_profile_meta(name, description);
    Ok(view(&engine))
}

#[tauri::command]
pub fn add_instance(
    engine: State<'_, SharedEngine>,
    instance_type: String,
) -> Result<AppStateView, String> {
    let t = match instance_type.as_str() {
        "mqtt_client" => InstanceType::MqttClient,
        "modbus_tcp_client" => InstanceType::ModbusTcpClient,
        "modbus_tcp_slave" => InstanceType::ModbusTcpSlave,
        other => return Err(format!("未知类型: {other}")),
    };
    engine.add_default(t, None);
    Ok(view(&engine))
}

#[tauri::command]
pub fn remove_instance(
    engine: State<'_, SharedEngine>,
    instance_id: String,
) -> Result<AppStateView, String> {
    engine.remove_instance(&instance_id);
    Ok(view(&engine))
}

#[tauri::command]
pub fn clone_instance(
    engine: State<'_, SharedEngine>,
    instance_id: String,
) -> Result<AppStateView, String> {
    engine
        .clone_instance(&instance_id)
        .ok_or_else(|| "实例不存在".to_string())?;
    Ok(view(&engine))
}

#[derive(Clone, Serialize)]
pub struct UpdateInstanceResult {
    #[serde(flatten)]
    pub state: AppStateView,
    pub warning: Option<String>,
}

#[tauri::command]
pub fn update_instance(
    engine: State<'_, SharedEngine>,
    instance: SimInstance,
) -> Result<UpdateInstanceResult, String> {
    let warning = engine.replace_instance(instance).map_err(map_err)?;
    Ok(UpdateInstanceResult {
        state: view(&engine),
        warning,
    })
}

#[tauri::command]
pub fn start_instance(
    engine: State<'_, SharedEngine>,
    instance_id: String,
) -> Result<AppStateView, String> {
    engine.start_instance(&instance_id).map_err(map_err)?;
    Ok(view(&engine))
}

#[tauri::command]
pub fn stop_instance(
    engine: State<'_, SharedEngine>,
    instance_id: String,
) -> Result<AppStateView, String> {
    engine.stop_instance(&instance_id);
    Ok(view(&engine))
}

#[tauri::command]
pub fn start_all(engine: State<'_, SharedEngine>) -> Result<AppStateView, String> {
    engine.start_all().map_err(map_err)?;
    Ok(view(&engine))
}

#[tauri::command]
pub fn stop_all(engine: State<'_, SharedEngine>) -> Result<AppStateView, String> {
    engine.stop_all();
    Ok(view(&engine))
}

#[tauri::command]
pub fn trigger_once(
    engine: State<'_, SharedEngine>,
    instance_id: String,
    rule_name: Option<String>,
) -> Result<(), String> {
    engine
        .trigger_once(&instance_id, rule_name)
        .map_err(map_err)
}

#[tauri::command]
pub fn profile_schema() -> serde_json::Value {
    let schema = schema_for!(Profile);
    serde_json::to_value(schema).unwrap_or_default()
}

pub fn build_engine(app: AppHandle) -> SharedEngine {
    let emit_app = app.clone();
    let engine = Arc::new(crate::engine::Engine::new(Arc::new(move |event| {
        let _ = emit_app.emit("sim-event", event);
    })));

    // try load last profile
    if let Some(home) = dirs_next_home() {
        let last = home.join(".devicesim").join("profile.json");
        if last.exists() {
            let _ = engine.load_profile(&last);
        }
    }
    engine
}

fn dirs_next_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}
