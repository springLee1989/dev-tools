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

    fn table_len(&self, table: &ModbusTable) -> usize {
        match table {
            ModbusTable::Coils => self.coils.lock().len(),
            ModbusTable::DiscreteInputs => self.discrete.lock().len(),
            ModbusTable::HoldingRegisters => self.holding.lock().len(),
            ModbusTable::InputRegisters => self.input.lock().len(),
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
        format!(
            "从站监听 {}:{} unit={} {}",
            cfg.host,
            cfg.port,
            cfg.unit_id,
            offset_listen_note(&cfg)
        ),
        None,
    ));

    let mut gens: Vec<_> = cfg
        .registers
        .iter()
        .filter(|r| r.enabled)
        .map(|r| (r.clone(), FieldGenerator::new(r.field.clone())))
        .collect();
    let mut due: std::collections::HashMap<String, Instant> = std::collections::HashMap::new();
    let mut warned: std::collections::HashSet<String> = std::collections::HashSet::new();
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
                    let schedule_next = |due: &mut std::collections::HashMap<String, Instant>| {
                        let jitter = if reg.jitter_ms != 0 {
                            rand::thread_rng().gen_range(-reg.jitter_ms..=reg.jitter_ms)
                        } else { 0 };
                        let next = (reg.interval_ms as i64 + jitter).max(50) as u64;
                        due.insert(reg.name.clone(), now + Duration::from_millis(next));
                    };
                    let offset = cfg.offset_for(&reg.table);
                    let Some(addr) = protocol_address(reg.address, offset) else {
                        if warned.insert(reg.name.clone()) {
                            emit(SimEvent::new(
                                &instance.id,
                                &instance.name,
                                "error",
                                "error",
                                format!(
                                    "寄存器 {} 地址 {} 偏移 {} 换算后的协议地址超出 0–65535",
                                    reg.name, reg.address, offset
                                ),
                                None,
                            ));
                        }
                        schedule_next(&mut due);
                        continue;
                    };
                    let value = gen.next(&ctx);
                    let encoded = encode_modbus(&value, &reg.data_type, &reg.byte_order);
                    let as_bool = reg.data_type == FieldType::Bool;
                    let span = encoded.len();
                    let len = memory.table_len(&reg.table);
                    if addr as usize + span > len {
                        if warned.insert(reg.name.clone()) {
                            emit(SimEvent::new(
                                &instance.id,
                                &instance.name,
                                "error",
                                "error",
                                format!(
                                    "寄存器 {} 协议地址 {} 长度 {} 超出数据区（大小 {}）",
                                    reg.name, addr, span, len
                                ),
                                None,
                            ));
                        }
                        schedule_next(&mut due);
                        continue;
                    }
                    memory.write_table(&reg.table, addr, &encoded, as_bool);
                    stats.lock().sent += 1;
                    stats.lock().last_activity = Some(now_secs());
                    schedule_next(&mut due);
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
            let resp = process_request(frame, &memory, unit_id);
            let req_hex = hex_spaced(frame);
            let req_text = describe_request(frame);
            let (resp_hex, resp_text) = match resp.as_deref() {
                Some(resp) => (hex_spaced(resp), describe_response(resp, frame)),
                None => (String::new(), "未应答".into()),
            };
            let message = if resp.is_some() {
                format!("收到 {req_hex} | {req_text}；应答 {resp_hex} | {resp_text}")
            } else {
                format!("收到 {req_hex} | {req_text}；{resp_text}")
            };
            emit(SimEvent::new(
                inst_id,
                inst_name,
                "info",
                "recv",
                message,
                Some(serde_json::json!({
                    "request_hex": req_hex,
                    "request": req_text,
                    "response_hex": resp_hex,
                    "response": resp_text,
                })),
            ));
            if let Some(resp) = resp {
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

fn hex_spaced(bytes: &[u8]) -> String {
    let text = hex::encode_upper(bytes);
    text.as_bytes()
        .chunks(2)
        .map(|c| std::str::from_utf8(c).unwrap_or(""))
        .collect::<Vec<_>>()
        .join(" ")
}

fn be_u16(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

fn fc_name(fc: u8) -> &'static str {
    match fc {
        0x01 => "读线圈",
        0x02 => "读离散输入",
        0x03 => "读保持寄存器",
        0x04 => "读输入寄存器",
        0x05 => "写单个线圈",
        0x06 => "写单个寄存器",
        0x0F => "写多个线圈",
        0x10 => "写多个寄存器",
        _ => "未知功能",
    }
}

fn exception_name(code: u8) -> &'static str {
    match code {
        0x01 => "非法功能",
        0x02 => "非法数据地址",
        0x03 => "非法数据值",
        0x04 => "从站设备故障",
        _ => "未知异常",
    }
}

fn fmt_capped(items: &[String]) -> String {
    const MAX: usize = 32;
    if items.len() <= MAX {
        format!("[{}]", items.join(", "))
    } else {
        format!("[{}, …] 共{}个", items[..MAX].join(", "), items.len())
    }
}

fn fmt_bools(bits: &[bool]) -> String {
    fmt_capped(
        &bits
            .iter()
            .map(|b| if *b { "true" } else { "false" }.to_string())
            .collect::<Vec<_>>(),
    )
}

fn fmt_u16s(regs: &[u16]) -> String {
    fmt_capped(&regs.iter().map(|v| v.to_string()).collect::<Vec<_>>())
}

fn bits_from(data: &[u8], qty: usize) -> Vec<bool> {
    (0..qty)
        .map(|i| data.get(i / 8).is_some_and(|b| b & (1 << (i % 8)) != 0))
        .collect()
}

fn regs_from(data: &[u8]) -> Vec<u16> {
    data.chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| u16::from_be_bytes([c[0], c[1]]))
        .collect()
}

fn describe_request(frame: &[u8]) -> String {
    if frame.len() < 8 {
        return "帧长度不足".into();
    }
    let tid = u16::from_be_bytes([frame[0], frame[1]]);
    let unit = frame[6];
    format!(
        "事务={tid} 单元={unit} {}",
        describe_request_pdu(&frame[7..])
    )
}

fn describe_request_pdu(pdu: &[u8]) -> String {
    let Some(&fc) = pdu.first() else {
        return "空 PDU".into();
    };
    let name = fc_name(fc);
    match fc {
        0x01 | 0x02 | 0x03 | 0x04 => match (be_u16(pdu, 1), be_u16(pdu, 3)) {
            (Some(addr), Some(qty)) => format!("功能={fc:02X} {name} 地址={addr} 数量={qty}"),
            _ => format!("功能={fc:02X} {name} 数据不完整"),
        },
        0x05 => match (be_u16(pdu, 1), be_u16(pdu, 3)) {
            (Some(addr), Some(val)) => {
                let on = match val {
                    0xFF00 => "ON",
                    0x0000 => "OFF",
                    _ => "非法",
                };
                format!("功能=05 {name} 地址={addr} 值={on} (0x{val:04X})")
            }
            _ => format!("功能=05 {name} 数据不完整"),
        },
        0x06 => match (be_u16(pdu, 1), be_u16(pdu, 3)) {
            (Some(addr), Some(val)) => format!("功能=06 {name} 地址={addr} 值={val}"),
            _ => format!("功能=06 {name} 数据不完整"),
        },
        0x0F => match (be_u16(pdu, 1), be_u16(pdu, 3)) {
            (Some(addr), Some(qty)) => {
                let bc = pdu.get(5).copied().unwrap_or(0) as usize;
                let data = pdu.get(6..6 + bc).unwrap_or(&[]);
                let bits = bits_from(data, qty as usize);
                format!(
                    "功能=0F {name} 地址={addr} 数量={qty} 值={}",
                    fmt_bools(&bits)
                )
            }
            _ => format!("功能=0F {name} 数据不完整"),
        },
        0x10 => match (be_u16(pdu, 1), be_u16(pdu, 3)) {
            (Some(addr), Some(qty)) => {
                let bc = pdu.get(5).copied().unwrap_or(0) as usize;
                let data = pdu.get(6..6 + bc).unwrap_or(&[]);
                let regs = regs_from(data);
                let regs = if qty as usize <= regs.len() {
                    &regs[..qty as usize]
                } else {
                    &regs[..]
                };
                format!(
                    "功能=10 {name} 地址={addr} 数量={qty} 值={}",
                    fmt_u16s(regs)
                )
            }
            _ => format!("功能=10 {name} 数据不完整"),
        },
        _ => format!("功能={fc:02X} {name}"),
    }
}

fn describe_response(frame: &[u8], request: &[u8]) -> String {
    if frame.len() < 8 {
        return "帧长度不足".into();
    }
    describe_response_pdu(&frame[7..], request.get(7..).unwrap_or(&[]))
}

fn describe_response_pdu(pdu: &[u8], request_pdu: &[u8]) -> String {
    let Some(&fc) = pdu.first() else {
        return "空 PDU".into();
    };
    if fc & 0x80 != 0 {
        let base = fc & 0x7F;
        let code = pdu.get(1).copied().unwrap_or(0);
        return format!(
            "功能={base:02X} {} 异常={code:02X} {}",
            fc_name(base),
            exception_name(code)
        );
    }
    let qty_hint = be_u16(request_pdu, 3).map(|q| q as usize);
    match fc {
        0x01 | 0x02 => {
            let bc = pdu.get(1).copied().unwrap_or(0) as usize;
            let data = pdu.get(2..).unwrap_or(&[]);
            let data = &data[..data.len().min(bc)];
            let qty = qty_hint.unwrap_or(data.len() * 8).min(data.len() * 8);
            format!(
                "功能={fc:02X} {} 值={}",
                fc_name(fc),
                fmt_bools(&bits_from(data, qty))
            )
        }
        0x03 | 0x04 => {
            let bc = pdu.get(1).copied().unwrap_or(0) as usize;
            let data = pdu.get(2..).unwrap_or(&[]);
            let data = &data[..data.len().min(bc)];
            let mut regs = regs_from(data);
            if let Some(qty) = qty_hint {
                regs.truncate(qty);
            }
            format!("功能={fc:02X} {} 值={}", fc_name(fc), fmt_u16s(&regs))
        }
        0x05 | 0x06 => describe_request_pdu(pdu),
        0x0F | 0x10 => match (be_u16(pdu, 1), be_u16(pdu, 3)) {
            (Some(addr), Some(qty)) => {
                format!("功能={fc:02X} {} 地址={addr} 数量={qty}", fc_name(fc))
            }
            _ => format!("功能={fc:02X} {} 数据不完整", fc_name(fc)),
        },
        _ => format!("功能={fc:02X} {}", fc_name(fc)),
    }
}

fn offset_listen_note(cfg: &ModbusTcpSlaveConfig) -> String {
    let skipped: Vec<&str> = [
        (!cfg.coils_use_offset, "线圈"),
        (!cfg.discrete_inputs_use_offset, "离散输入"),
        (!cfg.holding_registers_use_offset, "保持寄存器"),
        (!cfg.input_registers_use_offset, "输入寄存器"),
    ]
    .into_iter()
    .filter(|(skip, _)| *skip)
    .map(|(_, name)| name)
    .collect();
    if skipped.is_empty() {
        format!("地址偏移={}", cfg.address_offset)
    } else {
        format!(
            "地址偏移={}（{}不使用）",
            cfg.address_offset,
            skipped.join("、")
        )
    }
}

/// 协议地址 = 寄存器地址 − 偏移量，结果必须落在 Modbus PDU 的 0–65535。
fn protocol_address(address: u32, offset: i32) -> Option<u16> {
    let proto = address as i64 - i64::from(offset);
    u16::try_from(proto).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ModbusTable;

    #[test]
    fn missing_offset_in_old_profile_is_zero() {
        let cfg: ModbusTcpSlaveConfig =
            serde_json::from_str(r#"{"host":"0.0.0.0","port":5020,"unit_id":1}"#).unwrap();
        assert_eq!(cfg.address_offset, 0);
    }

    #[test]
    fn table_can_skip_address_offset() {
        let cfg = ModbusTcpSlaveConfig {
            address_offset: 40001,
            coils_use_offset: false,
            ..Default::default()
        };
        assert_eq!(cfg.offset_for(&ModbusTable::HoldingRegisters), 40001);
        assert_eq!(cfg.offset_for(&ModbusTable::Coils), 0);
        assert_eq!(
            protocol_address(40200, cfg.offset_for(&ModbusTable::HoldingRegisters)),
            Some(199)
        );
        assert_eq!(
            protocol_address(8, cfg.offset_for(&ModbusTable::Coils)),
            Some(8)
        );

        let loaded: ModbusTcpSlaveConfig = serde_json::from_str(r#"{"address_offset":1}"#).unwrap();
        assert!(loaded.coils_use_offset);
        assert_eq!(loaded.offset_for(&ModbusTable::InputRegisters), 1);
    }

    #[test]
    fn protocol_address_matches_plc_offset() {
        assert_eq!(protocol_address(200, 0), Some(200));
        assert_eq!(protocol_address(1, 1), Some(0));
        assert_eq!(protocol_address(40200, 40001), Some(199));
        assert_eq!(protocol_address(400201, 400001), Some(200));
        assert_eq!(protocol_address(0, 1), None);
        assert_eq!(protocol_address(0, -1), Some(1));
        assert_eq!(protocol_address(65535, -1), None);
    }

    #[test]
    fn simulated_value_is_read_at_protocol_address() {
        let cfg = ModbusTcpSlaveConfig {
            holding_registers_size: 300,
            address_offset: 40001,
            ..Default::default()
        };
        let mem = Memory::new(&cfg);
        let addr = protocol_address(40200, cfg.address_offset).unwrap();
        mem.write_table(&ModbusTable::HoldingRegisters, addr, &[0x1234], false);

        let mut frame = vec![0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01, 0x03];
        frame.extend_from_slice(&addr.to_be_bytes());
        frame.extend_from_slice(&1u16.to_be_bytes());
        let resp = process_request(&frame, &mem, 1).unwrap();
        assert_eq!(&resp[resp.len() - 2..], &0x1234u16.to_be_bytes());
    }

    #[test]
    fn log_line_contains_hex_and_parsed_fields() {
        let req = [
            0x00, 0x01, 0x00, 0x00, 0x00, 0x06, 0x01, 0x03, 0x00, 0xC8, 0x00, 0x02,
        ];
        let req_text = describe_request(&req);
        assert!(req_text.contains("事务=1"));
        assert!(req_text.contains("单元=1"));
        assert!(req_text.contains("功能=03 读保持寄存器 地址=200 数量=2"));

        let resp = [
            0x00, 0x01, 0x00, 0x00, 0x00, 0x07, 0x01, 0x03, 0x04, 0x00, 0x0A, 0x01, 0x00,
        ];
        let resp_text = describe_response(&resp, &req);
        assert_eq!(resp_text, "功能=03 读保持寄存器 值=[10, 256]");

        let line = format!(
            "收到 {} | {req_text}；应答 {} | {resp_text}",
            hex_spaced(&req),
            hex_spaced(&resp)
        );
        assert!(line.contains("00 01 00 00 00 06 01 03 00 C8 00 02"));
        assert!(line.contains("读保持寄存器 地址=200 数量=2"));
        assert!(line.contains("00 0A 01 00"));
        assert!(line.contains("值=[10, 256]"));

        let ex = [0x00, 0x01, 0x00, 0x00, 0x00, 0x03, 0x01, 0x83, 0x02];
        assert_eq!(
            describe_response(&ex, &req),
            "功能=03 读保持寄存器 异常=02 非法数据地址"
        );
    }
}
