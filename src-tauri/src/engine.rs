use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;

use crate::events::{EmitFn, InstanceStats};
use crate::modbus_client::ModbusTcpClientRunner;
use crate::modbus_slave::ModbusTcpSlaveRunner;
use crate::models::{
    default_modbus_client_instance, default_modbus_slave_instance, default_mqtt_instance,
    default_profile, InstanceType, Profile, SimInstance,
};
use crate::mqtt_client::MqttClientRunner;

enum Runner {
    Mqtt(MqttClientRunner),
    ModbusClient(ModbusTcpClientRunner),
    ModbusSlave(ModbusTcpSlaveRunner),
}

impl Runner {
    fn stop(&self) {
        match self {
            Runner::Mqtt(r) => r.stop(),
            Runner::ModbusClient(r) => r.stop(),
            Runner::ModbusSlave(r) => r.stop(),
        }
    }

    fn is_running(&self) -> bool {
        match self {
            Runner::Mqtt(r) => r.is_running(),
            Runner::ModbusClient(r) => r.is_running(),
            Runner::ModbusSlave(r) => r.is_running(),
        }
    }

    fn stats(&self) -> InstanceStats {
        match self {
            Runner::Mqtt(r) => r.stats(),
            Runner::ModbusClient(r) => r.stats(),
            Runner::ModbusSlave(r) => r.stats(),
        }
    }

    fn trigger_once(&self, rule_name: Option<String>) -> anyhow::Result<()> {
        match self {
            Runner::Mqtt(r) => r.publish_once(rule_name),
            Runner::ModbusClient(r) => r.trigger_once(rule_name),
            Runner::ModbusSlave(_) => Err(anyhow::anyhow!("从站不支持立即发送")),
        }
    }
}

pub struct Engine {
    profile: RwLock<Profile>,
    profile_path: RwLock<Option<PathBuf>>,
    runners: RwLock<HashMap<String, Runner>>,
    emit: RwLock<EmitFn>,
}

impl Engine {
    pub fn new(emit: EmitFn) -> Self {
        Self {
            profile: RwLock::new(default_profile()),
            profile_path: RwLock::new(None),
            runners: RwLock::new(HashMap::new()),
            emit: RwLock::new(emit),
        }
    }

    pub fn set_emit(&self, emit: EmitFn) {
        *self.emit.write() = emit;
    }

    pub fn profile(&self) -> Profile {
        self.profile.read().clone()
    }

    pub fn profile_path(&self) -> Option<PathBuf> {
        self.profile_path.read().clone()
    }

    pub fn load_profile(&self, path: impl AsRef<Path>) -> anyhow::Result<Profile> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path)?;
        let mut profile: Profile = serde_json::from_str(&text)?;
        for inst in &mut profile.instances {
            if inst.id.is_empty() {
                inst.id = crate::models::new_id();
            }
            inst.ensure_config();
        }
        self.stop_all();
        *self.profile.write() = profile.clone();
        *self.profile_path.write() = Some(path.to_path_buf());
        Ok(profile)
    }

    pub fn load_profile_data(&self, data: &str) -> anyhow::Result<Profile> {
        let mut profile: Profile = serde_json::from_str(data)?;
        for inst in &mut profile.instances {
            if inst.id.is_empty() {
                inst.id = crate::models::new_id();
            }
            inst.ensure_config();
        }
        self.stop_all();
        *self.profile.write() = profile.clone();
        Ok(profile)
    }

    pub fn save_profile(&self, path: Option<PathBuf>) -> anyhow::Result<PathBuf> {
        let path = path
            .or_else(|| self.profile_path.read().clone())
            .ok_or_else(|| anyhow::anyhow!("未指定保存路径"))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = serde_json::to_string_pretty(&*self.profile.read())?;
        std::fs::write(&path, text)?;
        *self.profile_path.write() = Some(path.clone());
        Ok(path)
    }

    pub fn dump_profile(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string_pretty(&*self.profile.read())?)
    }

    pub fn set_profile_meta(&self, name: String, description: String) {
        let mut p = self.profile.write();
        p.name = name;
        p.description = description;
    }

    pub fn add_default(&self, type_: InstanceType, name: Option<String>) -> SimInstance {
        let count = self.profile.read().instances.len() + 1;
        let inst = match type_ {
            InstanceType::MqttClient => {
                default_mqtt_instance(&name.unwrap_or_else(|| format!("mqtt-{count}")))
            }
            InstanceType::ModbusTcpClient => {
                default_modbus_client_instance(&name.unwrap_or_else(|| format!("mb-master-{count}")))
            }
            InstanceType::ModbusTcpSlave => {
                let used: std::collections::HashSet<u16> = self
                    .profile
                    .read()
                    .instances
                    .iter()
                    .filter_map(|i| i.modbus_slave.as_ref().map(|s| s.port))
                    .collect();
                let mut port = 5020;
                while used.contains(&port) {
                    port += 1;
                }
                default_modbus_slave_instance(
                    &name.unwrap_or_else(|| format!("mb-device-{count}")),
                    port,
                )
            }
        };
        self.profile.write().instances.push(inst.clone());
        inst
    }

    pub fn remove_instance(&self, instance_id: &str) {
        self.stop_instance(instance_id);
        self.profile
            .write()
            .instances
            .retain(|i| i.id != instance_id);
    }

    pub fn clone_instance(&self, instance_id: &str) -> Option<SimInstance> {
        let src = self.get_instance(instance_id)?;
        let mut data = serde_json::to_value(&src).ok()?;
        if let Some(obj) = data.as_object_mut() {
            obj.remove("id");
            obj.insert(
                "name".into(),
                serde_json::Value::String(format!("{}-copy", src.name)),
            );
        }
        let mut cloned: SimInstance = serde_json::from_value(data).ok()?;
        cloned.id = crate::models::new_id();
        if let Some(slave) = cloned.modbus_slave.as_mut() {
            let used: std::collections::HashSet<u16> = self
                .profile
                .read()
                .instances
                .iter()
                .filter_map(|i| i.modbus_slave.as_ref().map(|s| s.port))
                .collect();
            let mut port = slave.port + 1;
            while used.contains(&port) {
                port += 1;
            }
            slave.port = port;
        }
        self.profile.write().instances.push(cloned.clone());
        Some(cloned)
    }

    pub fn get_instance(&self, instance_id: &str) -> Option<SimInstance> {
        self.profile
            .read()
            .instances
            .iter()
            .find(|i| i.id == instance_id)
            .cloned()
    }

    /// 更新实例配置。若实例正在运行会先停再启；重启失败时配置仍保留，返回警告信息。
    pub fn replace_instance(&self, instance: SimInstance) -> anyhow::Result<Option<String>> {
        let running = self.is_running(&instance.id);
        if running {
            self.stop_instance(&instance.id);
        }
        {
            let mut profile = self.profile.write();
            if let Some(slot) = profile.instances.iter_mut().find(|i| i.id == instance.id) {
                *slot = instance.clone();
            } else {
                anyhow::bail!("实例不存在");
            }
        }
        if running {
            if let Err(err) = self.start_instance(&instance.id) {
                let msg = format!("配置已更新，但重新启动失败: {err}");
                let emit = self.emit.read().clone();
                emit(crate::events::SimEvent::new(
                    &instance.id,
                    &instance.name,
                    "error",
                    "error",
                    msg.clone(),
                    None,
                ));
                return Ok(Some(msg));
            }
        }
        Ok(None)
    }

    pub fn start_instance(&self, instance_id: &str) -> anyhow::Result<()> {
        let inst = self
            .get_instance(instance_id)
            .ok_or_else(|| anyhow::anyhow!("实例不存在: {instance_id}"))?;
        if !inst.enabled {
            anyhow::bail!("{} 已禁用", inst.name);
        }
        {
            let runners = self.runners.read();
            if let Some(r) = runners.get(instance_id) {
                if r.is_running() {
                    return Ok(());
                }
            }
        }
        let emit = self.emit.read().clone();
        let runner = match inst.instance_type {
            InstanceType::MqttClient => Runner::Mqtt(MqttClientRunner::start(inst, emit)?),
            InstanceType::ModbusTcpClient => {
                Runner::ModbusClient(ModbusTcpClientRunner::start(inst, emit)?)
            }
            InstanceType::ModbusTcpSlave => {
                Runner::ModbusSlave(ModbusTcpSlaveRunner::start(inst, emit)?)
            }
        };
        self.runners.write().insert(instance_id.to_string(), runner);
        Ok(())
    }

    pub fn stop_instance(&self, instance_id: &str) {
        if let Some(runner) = self.runners.write().remove(instance_id) {
            runner.stop();
        }
    }

    pub fn start_all(&self) -> anyhow::Result<()> {
        let ids: Vec<_> = self
            .profile
            .read()
            .instances
            .iter()
            .filter(|i| i.enabled)
            .map(|i| i.id.clone())
            .collect();
        for id in ids {
            self.start_instance(&id)?;
        }
        Ok(())
    }

    pub fn stop_all(&self) {
        let ids: Vec<_> = self.runners.read().keys().cloned().collect();
        for id in ids {
            self.stop_instance(&id);
        }
    }

    pub fn is_running(&self, instance_id: &str) -> bool {
        self.runners
            .read()
            .get(instance_id)
            .map(|r| r.is_running())
            .unwrap_or(false)
    }

    pub fn stats_of(&self, instance_id: &str) -> InstanceStats {
        self.runners
            .read()
            .get(instance_id)
            .map(|r| r.stats())
            .unwrap_or_default()
    }

    pub fn all_stats(&self) -> HashMap<String, InstanceStats> {
        let profile = self.profile.read();
        profile
            .instances
            .iter()
            .map(|i| (i.id.clone(), self.stats_of(&i.id)))
            .collect()
    }

    pub fn running_map(&self) -> HashMap<String, bool> {
        let profile = self.profile.read();
        profile
            .instances
            .iter()
            .map(|i| (i.id.clone(), self.is_running(&i.id)))
            .collect()
    }

    pub fn trigger_once(
        &self,
        instance_id: &str,
        rule_name: Option<String>,
    ) -> anyhow::Result<()> {
        let runners = self.runners.read();
        let runner = runners
            .get(instance_id)
            .ok_or_else(|| anyhow::anyhow!("实例未运行"))?;
        runner.trigger_once(rule_name)
    }
}

pub type SharedEngine = Arc<Engine>;
