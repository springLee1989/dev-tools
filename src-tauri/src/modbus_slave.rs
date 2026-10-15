use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::Rng;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::data_gen::{encode_modbus, FieldGenerator};
use crate::events::{now_secs, EmitFn, InstanceStats, SimEvent};
use crate::models::{FieldType, ModbusTable, ModbusTcpSlaveConfig, SimInstance};

#[derive(Clone)]
struct Memory {
    coils: Arc<Mutex<Vec<bool>>>,
    discrete: Arc<Mutex<Vec<bool>>>,
    holding: Arc<Mutex<Vec<u16>>>,
    input: Arc<Mutex<Vec<u16>>>,
}

impl Memory {
    fn new(cfg: &ModbusTcpSlaveConfig) -> Self {
        Self {
            coils: Arc::new(Mutex::new(vec![false; cfg.coils_size.max(16)])),
            discrete: Arc::new(Mutex::new(vec![false; cfg.discrete_inputs_size.max(16)])),
            holding: Arc::new(Mutex::new(vec![0; cfg.holding_registers_size.max(1)])),
            input: Arc::new(Mutex::new(vec![0; cfg.input_registers_size.max(1)])),
        }
    }

    fn write_table(&self, table: &ModbusTable, address: u16, values: &[u16], as_bool: bool) {
        match table {
            ModbusTable::Coils => {
                let mut g = self.coils.lock();
                for (i, v) in values.iter().enumerate() {
                    let idx = address as usize + i;
                    if idx < g.len() {
                        g[idx] = if as_bool { *v != 0 } else { *v != 0 };
                    }
                }
            }
            ModbusTable::DiscreteInputs => {
                let mut g = self.discrete.lock();
                for (i, v) in values.iter().enumerate() {
                    let idx = address as usize + i;
                    if idx < g.len() {
                        g[idx] = *v != 0;
                    }
                }
            }
            ModbusTable::HoldingRegisters => {
                let mut g = self.holding.lock();
                for (i, v) in values.iter().enumerate() {
                    let idx = address as usize + i;
                    if idx < g.len() {
                        g[idx] = *v;
                    }
                }
            }
            ModbusTable::InputRegisters => {
                let mut g = self.input.lock();
                for (i, v) in values.iter().enumerate() {
                    let idx = address as usize + i;
                    if idx < g.len() {
                        g[idx] = *v;
                    }
                }
            }
        }
    }
}

pub struct ModbusTcpSlaveRunner {
    stop_tx: watch::Sender<bool>,
    handle: Mutex<Option<JoinHandle<()>>>,
    stats: Arc<Mutex<InstanceStats>>,
    running: Arc<AtomicBool>,
}

impl ModbusTcpSlaveRunner {
    pub fn start(instance: SimInstance, emit: EmitFn) -> anyhow::Result<Self> {
        let cfg = instance
            .modbus_slave
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Modbus 从站配置缺失"))?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let stats = Arc::new(Mutex::new(InstanceStats {
            started_at: Some(now_secs()),
            ..Default::default()
        }));
        let running = Arc::new(AtomicBool::new(true));
        let stats_c = stats.clone();
        let running_c = running.clone();

        let handle = crate::runtime::spawn(async move {
            if let Err(err) = run_slave(instance, cfg, emit, stats_c.clone(), stop_rx).await {
                let mut st = stats_c.lock();
                st.errors += 1;
                st.last_error = err.to_string();
            }
            running_c.store(false, Ordering::SeqCst);
            stats_c.lock().connected = false;
        });

        Ok(Self {
            stop_tx,
            handle: Mutex::new(Some(handle)),
            stats,
            running,
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
}

async fn run_slave(
    instance: SimInstance,
    cfg: ModbusTcpSlaveConfig,
    emit: EmitFn,
    stats: Arc<Mutex<InstanceStats>>,
    mut stop_rx: watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let addr: SocketAddr = format!("{}:{}", cfg.host, cfg.port).parse()?;
    let listener = TcpListener::bind(addr).await?;
    let memory = Memory::new(&cfg);
    stats.lock().connected = true;
    emit(SimEvent::new(
        &instance.id,
        &instance.name,
        "info",
        "listen",
        format!("从站监听 {}:{} unit={}", cfg.host, cfg.port, cfg.unit_id),
        None,
    ));

    let mut gens: Vec<_> = cfg
        .registers
        .iter()
        .filter(|r| r.enabled)
        .map(|r| (r.clone(), FieldGenerator::new(r.field.clone())))
        .collect();
    let mut due: std::collections::HashMap<String, Instant> = std::collections::HashMap::new();
    let now = Instant::now();
    for (reg, _) in &gens {
        due.insert(reg.name.clone(), now);
    }

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
            }
            accept = listener.accept() => {
                match accept {
                    Ok((stream, peer)) => {
                        let mem = memory.clone();
                        let emit_c = emit.clone();
                        let stats_c = stats.clone();
                        let inst_id = instance.id.clone();
                        let inst_name = instance.name.clone();
                        let unit = cfg.unit_id;
                        let mut stop_c = stop_rx.clone();
                        crate::runtime::spawn(async move {
                            if let Err(err) = handle_conn(stream, mem, unit, emit_c.clone(), stats_c.clone(), &inst_id, &inst_name, &mut stop_c).await {
                                emit_c(SimEvent::new(&inst_id, &inst_name, "error", "error", format!("连接 {peer} 错误: {err}"), None));
                            }
                        });
                    }
                    Err(err) => {
                        stats.lock().errors += 1;
                        emit(SimEvent::new(&instance.id, &instance.name, "error", "error", format!("accept 失败: {err}"), None));
                    }
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                let now = Instant::now();
                let mut ctx = std::collections::HashMap::new();
                ctx.insert("name".into(), serde_json::Value::String(instance.name.clone()));
                ctx.insert("id".into(), serde_json::Value::String(instance.id.clone()));
                for (reg, gen) in &mut gens {
                    let due_at = due.get(&reg.name).copied().unwrap_or(now);
                    if now < due_at { continue; }
                    let value = gen.next(&ctx);
                    let encoded = encode_modbus(&value, &reg.data_type, &reg.byte_order);
                    let as_bool = reg.data_type == FieldType::Bool;
                    memory.write_table(&reg.table, reg.address, &encoded, as_bool);
                    stats.lock().sent += 1;
                    stats.lock().last_activity = Some(now_secs());
                    let jitter = if reg.jitter_ms != 0 {
                        rand::thread_rng().gen_range(-reg.jitter_ms..=reg.jitter_ms)
                    } else { 0 };
                    let next = (reg.interval_ms as i64 + jitter).max(50) as u64;
                    due.insert(reg.name.clone(), now + Duration::from_millis(next));
                }
            }
        }
    }
    Ok(())
}

async fn handle_conn(
    mut stream: TcpStream,
    memory: Memory,
    unit_id: u8,
    emit: EmitFn,
    stats: Arc<Mutex<InstanceStats>>,
    inst_id: &str,
    inst_name: &str,
    stop_rx: &mut watch::Receiver<bool>,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 512];
    loop {
        if *stop_rx.borrow() {
            break;
        }
        let n = tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() { break; }
                continue;
            }
            res = stream.read(&mut buf) => res?
        };
        if n == 0 {
            break;
        }
        let data = &buf[..n];
        if let Some(frame) = leading_tcp_frame(data) {
            stats.lock().received += 1;
            stats.lock().last_activity = Some(now_secs());
            let text = hex::encode_upper(frame);
            let spaced = text
                .as_bytes()
                .chunks(2)
                .map(|c| std::str::from_utf8(c).unwrap_or(""))
                .collect::<Vec<_>>()
                .join(" ");
            emit(SimEvent::new(
                inst_id,
                inst_name,
                "info",
                "recv",
                format!("收到 {spaced}"),
                Some(serde_json::json!(spaced)),
            ));
            if let Some(resp) = process_request(frame, &memory, unit_id) {
                stream.write_all(&resp).await?;
            }
        }
    }
    Ok(())
}

fn leading_tcp_frame(data: &[u8]) -> Option<&[u8]> {
    if data.len() < 8 || u16::from_be_bytes([data[2], data[3]]) != 0 {
        return None;
    }
    let mut msg_len = u16::from_be_bytes([data[4], data[5]]) as usize + 6;
    if msg_len == 8 && data.len() == 9 {
        msg_len = 9;
    }
    if msg_len < 8 || data.len() < msg_len {
        return None;
    }
    Some(&data[..msg_len])
}

fn process_request(frame: &[u8], memory: &Memory, unit_id: u8) -> Option<Vec<u8>> {
    if frame.len() < 8 {
        return None;
    }
    let tid = &frame[0..2];
    let unit = frame[6];
    let fc = frame[7];
    if unit != unit_id && unit != 0 {
        return None;
    }
    let pdu = &frame[7..];
    let resp_pdu = match fc {
        0x01 => read_bits(pdu, &memory.coils),
        0x02 => read_bits(pdu, &memory.discrete),
        0x03 => read_regs(pdu, &memory.holding),
        0x04 => read_regs(pdu, &memory.input),
        0x05 => write_single_coil(pdu, &memory.coils),
        0x06 => write_single_reg(pdu, &memory.holding),
        0x0F => write_multi_coils(pdu, &memory.coils),
        0x10 => write_multi_regs(pdu, &memory.holding),
        _ => exception(fc, 0x01),
    };
    let mut out = Vec::with_capacity(6 + resp_pdu.len());
    out.extend_from_slice(tid);
    out.extend_from_slice(&[0x00, 0x00]);
    let len = (resp_pdu.len() + 1) as u16;
    out.extend_from_slice(&len.to_be_bytes());
    out.push(unit);
    out.extend_from_slice(&resp_pdu);
    Some(out)
}

fn read_bits(pdu: &[u8], table: &Mutex<Vec<bool>>) -> Vec<u8> {
    if pdu.len() < 5 {
        return exception(pdu[0], 0x03);
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
    let qty = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
    let g = table.lock();
    if addr + qty > g.len() {
        return exception(pdu[0], 0x02);
    }
    let byte_count = qty.div_ceil(8);
    let mut data = vec![0u8; byte_count];
    for i in 0..qty {
        if g[addr + i] {
            data[i / 8] |= 1 << (i % 8);
        }
    }
    let mut out = vec![pdu[0], byte_count as u8];
    out.extend(data);
    out
}

fn read_regs(pdu: &[u8], table: &Mutex<Vec<u16>>) -> Vec<u8> {
    if pdu.len() < 5 {
        return exception(pdu[0], 0x03);
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
    let qty = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
    let g = table.lock();
    if addr + qty > g.len() {
        return exception(pdu[0], 0x02);
    }
    let mut out = vec![pdu[0], (qty * 2) as u8];
    for i in 0..qty {
        out.extend_from_slice(&g[addr + i].to_be_bytes());
    }
    out
}

fn write_single_coil(pdu: &[u8], table: &Mutex<Vec<bool>>) -> Vec<u8> {
    if pdu.len() < 5 {
        return exception(pdu[0], 0x03);
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
    let val = u16::from_be_bytes([pdu[3], pdu[4]]);
    let mut g = table.lock();
    if addr >= g.len() {
        return exception(pdu[0], 0x02);
    }
    g[addr] = val == 0xFF00;
    pdu[..5].to_vec()
}

fn write_single_reg(pdu: &[u8], table: &Mutex<Vec<u16>>) -> Vec<u8> {
    if pdu.len() < 5 {
        return exception(pdu[0], 0x03);
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
    let val = u16::from_be_bytes([pdu[3], pdu[4]]);
    let mut g = table.lock();
    if addr >= g.len() {
        return exception(pdu[0], 0x02);
    }
    g[addr] = val;
    pdu[..5].to_vec()
}

fn write_multi_coils(pdu: &[u8], table: &Mutex<Vec<bool>>) -> Vec<u8> {
    if pdu.len() < 6 {
        return exception(pdu[0], 0x03);
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
    let qty = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
    let bc = pdu[5] as usize;
    if pdu.len() < 6 + bc {
        return exception(pdu[0], 0x03);
    }
    let mut g = table.lock();
    if addr + qty > g.len() {
        return exception(pdu[0], 0x02);
    }
    for i in 0..qty {
        g[addr + i] = (pdu[6 + i / 8] & (1 << (i % 8))) != 0;
    }
    let mut out = vec![pdu[0]];
    out.extend_from_slice(&pdu[1..5]);
    out
}

fn write_multi_regs(pdu: &[u8], table: &Mutex<Vec<u16>>) -> Vec<u8> {
    if pdu.len() < 6 {
        return exception(pdu[0], 0x03);
    }
    let addr = u16::from_be_bytes([pdu[1], pdu[2]]) as usize;
    let qty = u16::from_be_bytes([pdu[3], pdu[4]]) as usize;
    let bc = pdu[5] as usize;
    if pdu.len() < 6 + bc || bc != qty * 2 {
        return exception(pdu[0], 0x03);
    }
    let mut g = table.lock();
    if addr + qty > g.len() {
        return exception(pdu[0], 0x02);
    }
    for i in 0..qty {
        g[addr + i] = u16::from_be_bytes([pdu[6 + i * 2], pdu[7 + i * 2]]);
    }
    let mut out = vec![pdu[0]];
    out.extend_from_slice(&pdu[1..5]);
    out
}

fn exception(fc: u8, code: u8) -> Vec<u8> {
    vec![fc | 0x80, code]
}
