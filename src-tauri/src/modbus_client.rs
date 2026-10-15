use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::Rng;
use serde_json::Value;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio_modbus::client::tcp;
use tokio_modbus::client::Context;
use tokio_modbus::prelude::*;

use crate::data_gen::{decode_modbus, encode_modbus, register_count, PayloadGenerator};
use crate::events::{now_secs, EmitFn, InstanceStats, SimEvent};
use crate::models::{
    FieldType, ModbusFunction, ModbusOperation, ModbusTcpClientConfig, SimInstance,
};

pub struct ModbusTcpClientRunner {
    stop_tx: watch::Sender<bool>,
    handle: Mutex<Option<JoinHandle<()>>>,
    stats: Arc<Mutex<InstanceStats>>,
    running: Arc<AtomicBool>,
    once_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Option<String>>>>>,
}

impl ModbusTcpClientRunner {
    pub fn start(instance: SimInstance, emit: EmitFn) -> anyhow::Result<Self> {
        let cfg = instance
            .modbus_client
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Modbus 客户端配置缺失"))?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let (once_tx, once_rx) = tokio::sync::mpsc::unbounded_channel();
        let stats = Arc::new(Mutex::new(InstanceStats {
            started_at: Some(now_secs()),
            ..Default::default()
        }));
        let running = Arc::new(AtomicBool::new(true));
        let stats_c = stats.clone();
        let running_c = running.clone();

        let handle = crate::runtime::spawn(async move {
            run_client(instance, cfg, emit, stats_c, stop_rx, once_rx).await;
            running_c.store(false, Ordering::SeqCst);
        });

        Ok(Self {
            stop_tx,
            handle: Mutex::new(Some(handle)),
            stats,
            running,
            once_tx: Arc::new(Mutex::new(Some(once_tx))),
        })
    }

    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
        if let Some(h) = self.handle.lock().take() {
            h.abort();
        }
        self.running.store(false, Ordering::SeqCst);
        self.stats.lock().connected = false;
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn stats(&self) -> InstanceStats {
        self.stats.lock().clone()
    }

    pub fn trigger_once(&self, op_name: Option<String>) -> anyhow::Result<()> {
        let guard = self.once_tx.lock();
        let tx = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("实例未运行"))?;
        tx.send(op_name)
            .map_err(|_| anyhow::anyhow!("发送通道已关闭"))?;
        Ok(())
    }
}

async fn run_client(
    instance: SimInstance,
    cfg: ModbusTcpClientConfig,
    emit: EmitFn,
    stats: Arc<Mutex<InstanceStats>>,
    mut stop_rx: watch::Receiver<bool>,
    mut once_rx: tokio::sync::mpsc::UnboundedReceiver<Option<String>>,
) {
    let mut ctx: Option<Context> = None;
    let mut due: HashMap<String, Instant> = HashMap::new();
    let now = Instant::now();
    for op in &cfg.operations {
        due.insert(op.name.clone(), now + Duration::from_millis(op.interval_ms));
    }

    loop {
        if *stop_rx.borrow() {
            break;
        }

        if ctx.is_none() {
            match connect(&cfg).await {
                Ok(c) => {
                    stats.lock().connected = true;
                    emit(SimEvent::new(
                        &instance.id,
                        &instance.name,
                        "info",
                        "connect",
                        format!("已连接 {}:{} unit={}", cfg.host, cfg.port, cfg.unit_id),
                        None,
                    ));
                    ctx = Some(c);
                }
                Err(err) => {
                    stats.lock().connected = false;
                    stats.lock().errors += 1;
                    stats.lock().last_error = err.to_string();
                    emit(SimEvent::new(
                        &instance.id,
                        &instance.name,
                        "error",
                        "error",
                        format!("无法连接 {}:{}: {err}", cfg.host, cfg.port),
                        None,
                    ));
                    if !cfg.auto_reconnect {
                        break;
                    }
                    tokio::select! {
                        _ = stop_rx.changed() => { if *stop_rx.borrow() { break; } }
                        _ = tokio::time::sleep(Duration::from_secs(1)) => {}
                    }
                    continue;
                }
            }
        }

        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
            }
            msg = once_rx.recv() => {
                if let Some(op_name) = msg {
                    let mut drop_conn = false;
                    if let Some(client) = ctx.as_mut() {
                        for op in &cfg.operations {
                            if !op.enabled { continue; }
                            if op_name.as_ref().map(|n| n == &op.name).unwrap_or(true) {
                                if let Err(err) = execute(client, &instance, &cfg, op, &emit, &stats).await {
                                    emit_error(&instance, &emit, &stats, &format!("{} 失败: {err}", op.name));
                                    if cfg.auto_reconnect {
                                        drop_conn = true;
                                    }
                                }
                            }
                        }
                    }
                    if drop_conn {
                        ctx = None;
                        stats.lock().connected = false;
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                let now = Instant::now();
                let mut drop_conn = false;
                if let Some(client) = ctx.as_mut() {
                    for op in &cfg.operations {
                        if !op.enabled { continue; }
                        let due_at = due.get(&op.name).copied().unwrap_or(now);
                        if now < due_at { continue; }
                        match execute(client, &instance, &cfg, op, &emit, &stats).await {
                            Ok(()) => {
                                let jitter = if op.jitter_ms != 0 {
                                    rand::thread_rng().gen_range(-op.jitter_ms..=op.jitter_ms)
                                } else { 0 };
                                let next = (op.interval_ms as i64 + jitter).max(50) as u64;
                                due.insert(op.name.clone(), now + Duration::from_millis(next));
                            }
                            Err(err) => {
                                emit_error(&instance, &emit, &stats, &format!("{} 失败: {err}", op.name));
                                if cfg.auto_reconnect {
                                    drop_conn = true;
                                }
                                break;
                            }
                        }
                    }
                }
                if drop_conn {
                    ctx = None;
                    stats.lock().connected = false;
                }
            }
        }
    }
    stats.lock().connected = false;
}

async fn connect(cfg: &ModbusTcpClientConfig) -> anyhow::Result<Context> {
    let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port).parse()?;
    let timeout = Duration::from_secs_f64(cfg.timeout.max(0.1));
    let mut ctx = tokio::time::timeout(timeout, tcp::connect(addr)).await??;
    ctx.set_slave(Slave(cfg.unit_id));
    Ok(ctx)
}

async fn execute(
    ctx: &mut Context,
    instance: &SimInstance,
    cfg: &ModbusTcpClientConfig,
    op: &ModbusOperation,
    emit: &EmitFn,
    stats: &Arc<Mutex<InstanceStats>>,
) -> anyhow::Result<()> {
    let is_write = matches!(
        op.function,
        ModbusFunction::WriteCoil
            | ModbusFunction::WriteRegister
            | ModbusFunction::WriteCoils
            | ModbusFunction::WriteRegisters
    );
    if is_write {
        let values = gen_write_values(instance, op);
        do_write(ctx, op, &values).await?;
        stats.lock().sent += 1;
        stats.lock().last_activity = Some(now_secs());
        emit(SimEvent::new(
            &instance.id,
            &instance.name,
            "info",
            "write",
            format!("{:?} addr={} values={:?}", op.function, op.address, values),
            Some(serde_json::json!(values)),
        ));
    } else {
        let decoded = do_read(ctx, op).await?;
        stats.lock().received += 1;
        stats.lock().last_activity = Some(now_secs());
        emit(SimEvent::new(
            &instance.id,
            &instance.name,
            "info",
            "read",
            format!("{:?} addr={} => {}", op.function, op.address, decoded),
            Some(decoded),
        ));
    }
    let _ = cfg;
    Ok(())
}

fn gen_write_values(instance: &SimInstance, op: &ModbusOperation) -> Vec<Value> {
    if !op.fields.is_empty() {
        let mut gen = PayloadGenerator::new(&op.fields);
        let mut ctx = HashMap::new();
        ctx.insert("name".into(), Value::String(instance.name.clone()));
        ctx.insert("id".into(), Value::String(instance.id.clone()));
        let obj = gen.next_object(&ctx);
        let mut values = Vec::new();
        for field in &op.fields {
            let v = obj.get(&field.name).cloned().unwrap_or(Value::Null);
            if matches!(
                op.function,
                ModbusFunction::WriteCoil | ModbusFunction::WriteCoils
            ) {
                values.push(Value::Bool(v.as_bool().unwrap_or(false)));
            } else {
                for reg in encode_modbus(&v, &field.field_type, &op.byte_order) {
                    values.push(serde_json::json!(reg));
                }
            }
        }
        if matches!(
            op.function,
            ModbusFunction::WriteCoil | ModbusFunction::WriteRegister
        ) {
            values.truncate(1);
        }
        return values;
    }
    if matches!(
        op.function,
        ModbusFunction::WriteCoil | ModbusFunction::WriteCoils
    ) {
        return vec![Value::Bool(true); op.count.max(1) as usize];
    }
    vec![serde_json::json!(0); op.count.max(1) as usize]
}

async fn do_read(ctx: &mut Context, op: &ModbusOperation) -> anyhow::Result<Value> {
    let count = op.count.max(1);
    match op.function {
        ModbusFunction::ReadCoils => {
            let bits = ctx.read_coils(op.address, count).await?.map_err(|e| anyhow::anyhow!("{e:?}"))?;
            Ok(serde_json::json!(bits.into_iter().take(count as usize).collect::<Vec<_>>()))
        }
        ModbusFunction::ReadDiscreteInputs => {
            let bits = ctx
                .read_discrete_inputs(op.address, count)
                .await?
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            Ok(serde_json::json!(bits.into_iter().take(count as usize).collect::<Vec<_>>()))
        }
        ModbusFunction::ReadHoldingRegisters => {
            let regs = ctx
                .read_holding_registers(op.address, count)
                .await?
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            Ok(decode_regs(op, &regs))
        }
        ModbusFunction::ReadInputRegisters => {
            let regs = ctx
                .read_input_registers(op.address, count)
                .await?
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
            Ok(decode_regs(op, &regs))
        }
        _ => Err(anyhow::anyhow!("非读功能码")),
    }
}

fn decode_regs(op: &ModbusOperation, registers: &[u16]) -> Value {
    let n = register_count(&op.data_type);
    if n == 1
        || matches!(
            op.data_type,
            FieldType::Bool | FieldType::Uint16 | FieldType::Int16 | FieldType::Int
        )
    {
        if op.data_type == FieldType::Int16 {
            let vals: Vec<Value> = registers
                .iter()
                .map(|r| decode_modbus(&[*r], &op.data_type, &op.byte_order))
                .collect();
            return Value::Array(vals);
        }
        return serde_json::json!(registers);
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i + n <= registers.len() {
        out.push(decode_modbus(&registers[i..i + n], &op.data_type, &op.byte_order));
        i += n;
    }
    Value::Array(out)
}

async fn do_write(ctx: &mut Context, op: &ModbusOperation, values: &[Value]) -> anyhow::Result<()> {
    match op.function {
        ModbusFunction::WriteCoil => {
            ctx.write_single_coil(
                op.address,
                values.first().and_then(|v| v.as_bool()).unwrap_or(false),
            )
            .await?
            .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        ModbusFunction::WriteRegister => {
            let v = values.first().and_then(|v| v.as_u64()).unwrap_or(0) as u16;
            ctx.write_single_register(op.address, v)
                .await?
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        ModbusFunction::WriteCoils => {
            let bits: Vec<bool> = values.iter().map(|v| v.as_bool().unwrap_or(false)).collect();
            ctx.write_multiple_coils(op.address, &bits)
                .await?
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        ModbusFunction::WriteRegisters => {
            let regs: Vec<u16> = values
                .iter()
                .map(|v| v.as_u64().unwrap_or(0) as u16)
                .collect();
            ctx.write_multiple_registers(op.address, &regs)
                .await?
                .map_err(|e| anyhow::anyhow!("{e:?}"))?;
        }
        _ => return Err(anyhow::anyhow!("非写功能码")),
    }
    Ok(())
}

fn emit_error(instance: &SimInstance, emit: &EmitFn, stats: &Arc<Mutex<InstanceStats>>, msg: &str) {
    stats.lock().errors += 1;
    stats.lock().last_error = msg.to_string();
    emit(SimEvent::new(
        &instance.id,
        &instance.name,
        "error",
        "error",
        msg,
        None,
    ));
}
