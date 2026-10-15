use std::collections::HashMap;
use std::sync::OnceLock;
use std::time::Instant;

use chrono::Utc;
use rand::seq::SliceRandom;
use rand::{thread_rng, Rng};
use regex::Regex;
use serde_json::{json, Map, Value};
use uuid::Uuid;

use crate::models::{ByteOrder, DataField, FieldType, GenMode};

fn token_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{([a-zA-Z0-9_]+)\}|\{\{([a-zA-Z0-9_]+)\}\}").unwrap())
}

fn int_range(ft: &FieldType) -> Option<(f64, f64)> {
    match ft {
        FieldType::Int16 => Some((-32768.0, 32767.0)),
        FieldType::Uint16 => Some((0.0, 65535.0)),
        FieldType::Int32 => Some((-2147483648.0, 2147483647.0)),
        FieldType::Uint32 => Some((0.0, 4294967295.0)),
        _ => None,
    }
}

pub fn render_template(template: &str, ctx: &HashMap<String, Value>) -> String {
    token_re()
        .replace_all(template, |caps: &regex::Captures| {
            let key = caps.get(1).or_else(|| caps.get(2)).map(|m| m.as_str()).unwrap_or("");
            match ctx.get(key) {
                Some(Value::Null) | None => caps.get(0).map(|m| m.as_str()).unwrap_or("").to_string(),
                Some(v) => value_to_string(v),
            }
        })
        .into_owned()
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn as_f64(v: &Option<Value>, default: f64) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::String(s)) => s.parse().unwrap_or(default),
        Some(Value::Bool(b)) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        _ => default,
    }
}

pub fn clamp_numeric(value: f64, field: &DataField) -> f64 {
    let mut lo = field.min;
    let mut hi = field.max;
    if let Some((rlo, rhi)) = int_range(&field.field_type) {
        lo = Some(lo.map(|v| v.max(rlo)).unwrap_or(rlo));
        hi = Some(hi.map(|v| v.min(rhi)).unwrap_or(rhi));
    }
    let mut value = value;
    if let Some(lo) = lo {
        value = value.max(lo);
    }
    if let Some(hi) = hi {
        value = value.min(hi);
    }
    value
}

fn cast_value(raw: Value, field: &DataField) -> Value {
    if field.mode == GenMode::Timestamp || field.mode == GenMode::Uuid {
        return raw;
    }
    match field.field_type {
        FieldType::Bool => match raw {
            Value::Bool(b) => Value::Bool(b),
            Value::String(s) => {
                let s = s.to_lowercase();
                Value::Bool(matches!(s.as_str(), "1" | "true" | "yes" | "on"))
            }
            Value::Number(n) => Value::Bool(n.as_f64().unwrap_or(0.0) != 0.0),
            _ => Value::Bool(false),
        },
        FieldType::String | FieldType::Hex => Value::String(value_to_string(&raw)),
        FieldType::Float | FieldType::Float32 => {
            let mut n = as_f64(&Some(raw), 0.0);
            if let Some(d) = field.decimals {
                let factor = 10f64.powi(d);
                n = (n * factor).round() / factor;
            }
            json!(n)
        }
        _ => {
            let n = clamp_numeric(as_f64(&Some(raw), 0.0), field).round() as i64;
            json!(n)
        }
    }
}

pub struct FieldGenerator {
    field: DataField,
    n: f64,
    t0: Instant,
}

impl FieldGenerator {
    pub fn new(field: DataField) -> Self {
        let mut n = field.start;
        if field.mode == GenMode::Walk {
            n = if field.start != 0.0 {
                field.start
            } else {
                field.min.unwrap_or(0.0)
            };
        }
        Self {
            field,
            n,
            t0: Instant::now(),
        }
    }

    pub fn next(&mut self, ctx: &HashMap<String, Value>) -> Value {
        let field = &self.field;
        let mut rng = thread_rng();
        let raw = match field.mode {
            GenMode::Const => field.value.clone().unwrap_or(Value::Null),
            GenMode::Random => self.random(&mut rng),
            GenMode::Gaussian => {
                let mu = field.mean.unwrap_or_else(|| {
                    (field.min.unwrap_or(0.0) + field.max.unwrap_or(0.0)) / 2.0
                });
                let sigma = field.std.unwrap_or(1.0);
                let raw = clamp_numeric(normal(&mut rng, mu, sigma), field);
                json!(raw)
            }
            GenMode::Increment => {
                let raw = self.n;
                self.n += field.step;
                if field.wrap {
                    if let Some(max) = field.max {
                        if self.n > max {
                            self.n = field.min.unwrap_or(field.start);
                        }
                    }
                }
                json!(raw)
            }
            GenMode::Sine => {
                let elapsed = self.t0.elapsed().as_secs_f64();
                let period = if field.period == 0.0 { 60.0 } else { field.period };
                let lo = field.min.unwrap_or(0.0);
                let hi = field.max.unwrap_or(1.0);
                let mid = (lo + hi) / 2.0;
                let amp = (hi - lo) / 2.0;
                json!(mid + amp * (2.0 * std::f64::consts::PI * elapsed / period).sin())
            }
            GenMode::Enum => {
                let values = if field.values.is_empty() {
                    field
                        .value
                        .clone()
                        .map(|v| vec![v])
                        .unwrap_or_default()
                } else {
                    field.values.clone()
                };
                if values.is_empty() {
                    Value::Null
                } else if !field.weights.is_empty() && field.weights.len() == values.len() {
                    weighted_choice(&mut rng, &values, &field.weights)
                } else {
                    values.choose(&mut rng).cloned().unwrap_or(Value::Null)
                }
            }
            GenMode::Walk => {
                let delta = rng.gen_range(-field.step.abs()..=field.step.abs());
                self.n = clamp_numeric(self.n + delta, field);
                json!(self.n)
            }
            GenMode::Timestamp => {
                let now = Utc::now();
                let fmt = if field.template.is_empty() {
                    "iso8601"
                } else {
                    field.template.as_str()
                };
                match fmt {
                    "iso8601" => Value::String(
                        now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
                    ),
                    "epoch" => json!(now.timestamp()),
                    "epoch_ms" => json!(now.timestamp_millis()),
                    other => Value::String(now.format(other).to_string()),
                }
            }
            GenMode::Uuid => Value::String(Uuid::new_v4().to_string()),
            GenMode::Template => {
                let tpl = if field.template.is_empty() {
                    field
                        .value
                        .as_ref()
                        .map(value_to_string)
                        .unwrap_or_default()
                } else {
                    field.template.clone()
                };
                Value::String(render_template(&tpl, ctx))
            }
        };

        let mut raw = raw;
        if field.field_type == FieldType::Bool && field.mode == GenMode::Random {
            raw = Value::Bool(rng.gen::<f64>() < field.probability);
        }
        if field.field_type == FieldType::Hex
            && matches!(field.mode, GenMode::Random | GenMode::Uuid)
        {
            let n = field.hex_bytes.max(1);
            let bytes: Vec<u8> = (0..n).map(|_| rng.gen()).collect();
            raw = Value::String(hex::encode(bytes));
        }
        if field.field_type == FieldType::String
            && field.mode == GenMode::Random
            && field.template.is_empty()
        {
            raw = Value::String(format!("{}-{:06}", field.name, rng.gen_range(0..1_000_000)));
        }

        cast_value(raw, field)
    }

    fn random(&self, rng: &mut impl Rng) -> Value {
        let field = &self.field;
        if field.field_type == FieldType::Bool {
            return Value::Bool(rng.gen::<f64>() < field.probability);
        }
        let mut lo = field.min.unwrap_or(0.0);
        let mut hi = field.max.unwrap_or(lo + 1.0);
        if hi < lo {
            std::mem::swap(&mut lo, &mut hi);
        }
        match field.field_type {
            FieldType::Float | FieldType::Float32 | FieldType::String => {
                json!(rng.gen_range(lo..=hi))
            }
            _ => {
                let lo_i = lo.round() as i64;
                let hi_i = hi.round() as i64;
                json!(rng.gen_range(lo_i..=hi_i))
            }
        }
    }
}

fn normal(rng: &mut impl Rng, mu: f64, sigma: f64) -> f64 {
    // Box-Muller
    let u1: f64 = rng.gen::<f64>().max(f64::EPSILON);
    let u2: f64 = rng.gen();
    let z = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
    mu + sigma * z
}

fn weighted_choice(rng: &mut impl Rng, values: &[Value], weights: &[f64]) -> Value {
    let total: f64 = weights.iter().sum();
    if total <= 0.0 {
        return values.choose(rng).cloned().unwrap_or(Value::Null);
    }
    let mut r = rng.gen::<f64>() * total;
    for (v, w) in values.iter().zip(weights.iter()) {
        if r < *w {
            return v.clone();
        }
        r -= w;
    }
    values.last().cloned().unwrap_or(Value::Null)
}

pub struct PayloadGenerator {
    gens: Vec<(String, FieldGenerator)>,
}

impl PayloadGenerator {
    pub fn new(fields: &[DataField]) -> Self {
        Self {
            gens: fields
                .iter()
                .map(|f| (f.name.clone(), FieldGenerator::new(f.clone())))
                .collect(),
        }
    }

    pub fn next_object(&mut self, ctx: &HashMap<String, Value>) -> Map<String, Value> {
        let mut base = ctx.clone();
        let mut out = Map::new();
        for (name, gen) in &mut self.gens {
            let value = gen.next(&base);
            out.insert(name.clone(), value.clone());
            base.insert(name.clone(), value);
        }
        out
    }
}

pub fn encode_modbus(value: &Value, data_type: &FieldType, byte_order: &ByteOrder) -> Vec<u16> {
    match data_type {
        FieldType::Bool => vec![if value.as_bool().unwrap_or(false) {
            1
        } else {
            0
        }],
        FieldType::Int16 | FieldType::Uint16 | FieldType::Int => {
            let n = as_f64(&Some(value.clone()), 0.0) as i64;
            vec![(n as u16)]
        }
        FieldType::String => {
            let mut data = value_to_string(value).into_bytes();
            if data.len() % 2 == 1 {
                data.push(0);
            }
            let mut regs = Vec::new();
            for chunk in data.chunks(2) {
                regs.push(((chunk[0] as u16) << 8) | chunk.get(1).copied().unwrap_or(0) as u16);
            }
            if regs.is_empty() {
                regs.push(0);
            }
            regs
        }
        FieldType::Int32 => {
            let n = as_f64(&Some(value.clone()), 0.0) as i32;
            bytes_to_regs32(&n.to_be_bytes(), byte_order)
        }
        FieldType::Uint32 => {
            let n = as_f64(&Some(value.clone()), 0.0) as u32;
            bytes_to_regs32(&n.to_be_bytes(), byte_order)
        }
        FieldType::Float | FieldType::Float32 => {
            let n = as_f64(&Some(value.clone()), 0.0) as f32;
            bytes_to_regs32(&n.to_be_bytes(), byte_order)
        }
        _ => {
            let n = as_f64(&Some(value.clone()), 0.0) as i64;
            vec![(n as u16)]
        }
    }
}

pub fn decode_modbus(registers: &[u16], data_type: &FieldType, byte_order: &ByteOrder) -> Value {
    match data_type {
        FieldType::Bool => Value::Bool(registers.first().copied().unwrap_or(0) != 0),
        FieldType::Uint16 | FieldType::Int => {
            json!(registers.first().copied().unwrap_or(0))
        }
        FieldType::Int16 => {
            let v = registers.first().copied().unwrap_or(0);
            let signed = if v & 0x8000 != 0 {
                (v as i32) - 0x10000
            } else {
                v as i32
            };
            json!(signed)
        }
        FieldType::String => {
            let mut data = Vec::new();
            for r in registers {
                data.push((r >> 8) as u8);
                data.push((r & 0xff) as u8);
            }
            let end = data.iter().position(|&b| b == 0).unwrap_or(data.len());
            Value::String(String::from_utf8_lossy(&data[..end]).into_owned())
        }
        FieldType::Int32 => {
            let raw = regs32_to_bytes(registers, byte_order);
            let n = i32::from_be_bytes(raw);
            json!(n)
        }
        FieldType::Uint32 => {
            let raw = regs32_to_bytes(registers, byte_order);
            let n = u32::from_be_bytes(raw);
            json!(n)
        }
        _ => {
            let raw = regs32_to_bytes(registers, byte_order);
            let n = f32::from_be_bytes(raw);
            json!(n)
        }
    }
}

pub fn register_count(data_type: &FieldType) -> usize {
    match data_type {
        FieldType::Int32 | FieldType::Uint32 | FieldType::Float | FieldType::Float32 => 2,
        _ => 1,
    }
}

fn bytes_to_regs32(raw: &[u8; 4], byte_order: &ByteOrder) -> Vec<u16> {
    let b = permute(*raw, byte_order);
    vec![
        ((b[0] as u16) << 8) | b[1] as u16,
        ((b[2] as u16) << 8) | b[3] as u16,
    ]
}

fn regs32_to_bytes(registers: &[u16], byte_order: &ByteOrder) -> [u8; 4] {
    let r0 = registers.first().copied().unwrap_or(0);
    let r1 = registers.get(1).copied().unwrap_or(0);
    let data = [
        (r0 >> 8) as u8,
        (r0 & 0xff) as u8,
        (r1 >> 8) as u8,
        (r1 & 0xff) as u8,
    ];
    unpermute(data, byte_order)
}

fn permute(abcd: [u8; 4], byte_order: &ByteOrder) -> [u8; 4] {
    let [a, b, c, d] = abcd;
    match byte_order {
        ByteOrder::ABCD => [a, b, c, d],
        ByteOrder::CDAB => [c, d, a, b],
        ByteOrder::BADC => [b, a, d, c],
        ByteOrder::DCBA => [d, c, b, a],
    }
}

fn unpermute(data: [u8; 4], byte_order: &ByteOrder) -> [u8; 4] {
    let labels = permute([b'A', b'B', b'C', b'D'], byte_order);
    let mut recovered = [0u8; 4];
    for (i, label) in labels.iter().enumerate() {
        let idx = match label {
            b'A' => 0,
            b'B' => 1,
            b'C' => 2,
            _ => 3,
        };
        recovered[idx] = data[i];
    }
    recovered
}

pub fn map_to_value(map: Map<String, Value>) -> Value {
    Value::Object(map)
}

