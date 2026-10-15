use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use rand::Rng;
use rumqttc::{AsyncClient, Event, Incoming, MqttOptions, QoS, Transport};
use serde_json::Value;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use crate::data_gen::{map_to_value, render_template, PayloadGenerator};
use crate::events::{now_secs, EmitFn, InstanceStats, SimEvent};
use crate::models::{MqttClientConfig, MqttPublishRule, SimInstance};

pub struct MqttClientRunner {
    stop_tx: watch::Sender<bool>,
    handle: Mutex<Option<JoinHandle<()>>>,
    stats: Arc<Mutex<InstanceStats>>,
    running: Arc<AtomicBool>,
    once_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<Option<String>>>>>,
}

impl MqttClientRunner {
    pub fn start(instance: SimInstance, emit: EmitFn) -> anyhow::Result<Self> {
        let cfg = instance
            .mqtt
            .clone()
            .ok_or_else(|| anyhow::anyhow!("MQTT 配置缺失"))?;
        let (stop_tx, stop_rx) = watch::channel(false);
        let (once_tx, once_rx) = tokio::sync::mpsc::unbounded_channel();
        let stats = Arc::new(Mutex::new(InstanceStats {
            started_at: Some(now_secs()),
            ..Default::default()
        }));
        let running = Arc::new(AtomicBool::new(true));
        let stats_c = stats.clone();
        let running_c = running.clone();
        let once_tx_store = Arc::new(Mutex::new(Some(once_tx)));

        let handle = crate::runtime::spawn(async move {
            if let Err(err) = run_mqtt(instance, cfg, emit, stats_c.clone(), stop_rx, once_rx).await
            {
                let mut st = stats_c.lock();
                st.errors += 1;
                st.last_error = err.to_string();
                st.connected = false;
            }
            running_c.store(false, Ordering::SeqCst);
        });

        Ok(Self {
            stop_tx,
            handle: Mutex::new(Some(handle)),
            stats,
            running,
            once_tx: once_tx_store,
        })
    }

    pub fn stop(&self) {
        let _ = self.stop_tx.send(true);
        if let Some(handle) = self.handle.lock().take() {
            handle.abort();
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

    pub fn publish_once(&self, rule_name: Option<String>) -> anyhow::Result<()> {
        let guard = self.once_tx.lock();
        let tx = guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("实例未运行"))?;
        tx.send(rule_name)
            .map_err(|_| anyhow::anyhow!("发送通道已关闭"))?;
        Ok(())
    }
}

async fn run_mqtt(
    instance: SimInstance,
    cfg: MqttClientConfig,
    emit: EmitFn,
    stats: Arc<Mutex<InstanceStats>>,
    mut stop_rx: watch::Receiver<bool>,
    mut once_rx: tokio::sync::mpsc::UnboundedReceiver<Option<String>>,
) -> anyhow::Result<()> {
    let client_id = if cfg.client_id.is_empty() {
        format!("devicesim-{}", instance.id)
    } else {
        cfg.client_id.clone()
    };
    let mut opts = MqttOptions::new(client_id, &cfg.host, cfg.port);
    opts.set_keep_alive(Duration::from_secs(cfg.keepalive.max(1)));
    opts.set_clean_session(cfg.clean_session);
    if !cfg.username.is_empty() {
        opts.set_credentials(&cfg.username, &cfg.password);
    }
    if cfg.use_tls {
        opts.set_transport(Transport::tls_with_default_config());
    }

    let base_ctx = base_ctx(&instance, &cfg);
    if !cfg.lwt_topic.is_empty() {
        let topic = render_template(&cfg.lwt_topic, &base_ctx);
        let payload = render_template(&cfg.lwt_payload, &base_ctx);
        opts.set_last_will(rumqttc::LastWill::new(
            topic,
            payload,
            qos_from(cfg.lwt_qos),
            cfg.lwt_retain,
        ));
    }

    let (client, mut eventloop) = AsyncClient::new(opts, 64);
    let mut publishers: Vec<(MqttPublishRule, PayloadGenerator)> = cfg
        .publishes
        .iter()
        .filter(|r| r.enabled)
        .map(|r| (r.clone(), PayloadGenerator::new(&r.fields)))
        .collect();

    let mut due: HashMap<String, Instant> = HashMap::new();
    let now = Instant::now();
    for (rule, _) in &publishers {
        let jitter_max = rule.jitter_ms.max(0);
        let delay = (rule.interval_ms as i64
            + if jitter_max > 0 {
                rand::thread_rng().gen_range(0..=jitter_max)
            } else {
                0
            }) as u64;
        due.insert(rule.name.clone(), now + Duration::from_millis(delay.max(50)));
    }

    let mut connected = false;
    let mut birth_done = false;

    loop {
        tokio::select! {
            _ = stop_rx.changed() => {
                if *stop_rx.borrow() {
                    let _ = client.disconnect().await;
                    break;
                }
            }
            msg = once_rx.recv() => {
                if let Some(rule_name) = msg {
                    if connected {
                        for (rule, gen) in &mut publishers {
                            if rule_name.as_ref().map(|n| n == &rule.name).unwrap_or(true) {
                                publish_one(&client, &instance, &cfg, rule, gen, &emit, &stats).await;
                            }
                        }
                    }
                }
            }
            event = eventloop.poll() => {
                match event {
                    Ok(Event::Incoming(Incoming::ConnAck(_))) => {
                        connected = true;
                        stats.lock().connected = true;
                        emit(SimEvent::new(
                            &instance.id,
                            &instance.name,
                            "info",
                            "connect",
                            format!("已连接 {}:{}", cfg.host, cfg.port),
                            None,
                        ));
                        if !birth_done && !cfg.birth_topic.is_empty() {
                            let topic = render_template(&cfg.birth_topic, &base_ctx);
                            let payload = render_template(&cfg.birth_payload, &base_ctx);
                            if client.publish(topic, qos_from(cfg.birth_qos), cfg.birth_retain, payload).await.is_ok() {
                                stats.lock().sent += 1;
                            }
                            birth_done = true;
                        }
                        for sub in &cfg.subscriptions {
                            if !sub.enabled { continue; }
                            let topic = render_template(&sub.topic, &base_ctx);
                            let _ = client.subscribe(&topic, qos_from(sub.qos)).await;
                            emit(SimEvent::new(
                                &instance.id,
                                &instance.name,
                                "info",
                                "subscribe",
                                format!("订阅 {topic} qos={}", sub.qos),
                                None,
                            ));
                        }
                    }
                    Ok(Event::Incoming(Incoming::Publish(p))) => {
                        stats.lock().received += 1;
                        stats.lock().last_activity = Some(now_secs());
                        let text = String::from_utf8(p.payload.to_vec())
                            .unwrap_or_else(|_| hex::encode(&p.payload));
                        emit(SimEvent::new(
                            &instance.id,
                            &instance.name,
                            "info",
                            "message",
                            format!("收到 {} ({}B)", p.topic, p.payload.len()),
                            Some(serde_json::json!({"topic": p.topic, "payload": text})),
                        ));
                    }
                    Ok(Event::Incoming(Incoming::Disconnect)) => {
                        connected = false;
                        stats.lock().connected = false;
                        emit(SimEvent::new(&instance.id, &instance.name, "info", "disconnect", "断开连接", None));
                        if !cfg.auto_reconnect {
                            break;
                        }
                    }
                    Err(err) => {
                        connected = false;
                        stats.lock().connected = false;
                        stats.lock().errors += 1;
                        stats.lock().last_error = err.to_string();
                        emit(SimEvent::new(
                            &instance.id,
                            &instance.name,
                            "error",
                            "error",
                            format!("MQTT 错误: {err}"),
                            None,
                        ));
                        if !cfg.auto_reconnect {
                            break;
                        }
                        tokio::time::sleep(Duration::from_secs(1)).await;
                    }
                    _ => {}
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(50)) => {
                if !connected { continue; }
                let now = Instant::now();
                for (rule, gen) in &mut publishers {
                    let due_at = due.get(&rule.name).copied().unwrap_or(now);
                    if now >= due_at {
                        publish_one(&client, &instance, &cfg, rule, gen, &emit, &stats).await;
                        let jitter = if rule.jitter_ms != 0 {
                            rand::thread_rng().gen_range(-rule.jitter_ms..=rule.jitter_ms)
                        } else {
                            0
                        };
                        let next = (rule.interval_ms as i64 + jitter).max(50) as u64;
                        due.insert(rule.name.clone(), now + Duration::from_millis(next));
                    }
                }
            }
        }
    }
    stats.lock().connected = false;
    Ok(())
}

async fn publish_one(
    client: &AsyncClient,
    instance: &SimInstance,
    cfg: &MqttClientConfig,
    rule: &MqttPublishRule,
    gen: &mut PayloadGenerator,
    emit: &EmitFn,
    stats: &Arc<Mutex<InstanceStats>>,
) {
    let mut ctx = base_ctx(instance, cfg);
    let obj = gen.next_object(&ctx);
    for (k, v) in &obj {
        ctx.insert(k.clone(), v.clone());
    }
    let topic = render_template(&rule.topic, &ctx);
    let payload_result: Result<Vec<u8>, String> = match rule.payload_kind.as_str() {
        "hex" => {
            let text = render_template(&rule.text_template, &ctx).replace(' ', "");
            hex::decode(text).map_err(|e| e.to_string())
        }
        "text" => Ok(render_template(&rule.text_template, &ctx).into_bytes()),
        _ => Ok(serde_json::to_vec(&map_to_value(obj.clone())).unwrap_or_default()),
    };
    match payload_result {
        Ok(payload) => {
            if let Err(err) = client
                .publish(&topic, qos_from(rule.qos), rule.retain, payload.clone())
                .await
            {
                stats.lock().errors += 1;
                stats.lock().last_error = err.to_string();
                emit(SimEvent::new(
                    &instance.id,
                    &instance.name,
                    "error",
                    "error",
                    format!("发布失败 {topic}: {err}"),
                    None,
                ));
                return;
            }
            stats.lock().sent += 1;
            stats.lock().last_activity = Some(now_secs());
            let mut preview = String::from_utf8(payload).unwrap_or_else(|e| hex::encode(e.as_bytes()));
            if preview.len() > 240 {
                preview.truncate(240);
                preview.push('…');
            }
            emit(SimEvent::new(
                &instance.id,
                &instance.name,
                "info",
                "publish",
                format!("发布 {topic} {preview}"),
                Some(map_to_value(obj)),
            ));
        }
        Err(err) => {
            stats.lock().errors += 1;
            stats.lock().last_error = err.clone();
            emit(SimEvent::new(
                &instance.id,
                &instance.name,
                "error",
                "error",
                format!("发布失败 {topic}: {err}"),
                None,
            ));
        }
    }
}

fn base_ctx(instance: &SimInstance, cfg: &MqttClientConfig) -> HashMap<String, Value> {
    let mut ctx = HashMap::new();
    ctx.insert("name".into(), Value::String(instance.name.clone()));
    ctx.insert("id".into(), Value::String(instance.id.clone()));
    ctx.insert(
        "type".into(),
        Value::String("mqtt_client".into()),
    );
    ctx.insert("host".into(), Value::String(cfg.host.clone()));
    ctx.insert("port".into(), serde_json::json!(cfg.port));
    ctx
}

fn qos_from(q: u8) -> QoS {
    match q {
        1 => QoS::AtLeastOnce,
        2 => QoS::ExactlyOnce,
        _ => QoS::AtMostOnce,
    }
}
