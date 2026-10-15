use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::{Parser, Subcommand};
use devicesim_lib::engine::Engine;
use devicesim_lib::events::{EmitFn, SimEvent};
use devicesim_lib::models::Profile;

#[derive(Parser)]
#[command(name = "devicesim", version = "1.0.0", about = "MQTT / Modbus TCP 多实例设备仿真工具")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// 无界面运行方案
    Run {
        profile: PathBuf,
        /// 运行秒数，0 表示直到 Ctrl+C
        #[arg(long, default_value_t = 0.0)]
        duration: f64,
    },
    /// 校验方案 JSON
    Validate { profile: PathBuf },
    /// 打印方案 JSON Schema
    Schema,
}

fn print_event(event: SimEvent) {
    let ts = chrono::DateTime::from_timestamp(event.ts as i64, 0)
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_else(|| "--:--:--".into());
    println!(
        "{ts} [{:<5}] {:<20} {:<10} {}",
        event.level, event.instance_name, event.kind, event.message
    );
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Run { profile, duration } => {
            let emit: EmitFn = Arc::new(print_event);
            let engine = Engine::new(emit);
            engine.load_profile(&profile)?;
            println!(
                "已加载方案: {} ({} 个实例)",
                engine.profile().name,
                engine.profile().instances.len()
            );
            engine.start_all()?;
            if duration > 0.0 {
                tokio::select! {
                    _ = tokio::time::sleep(Duration::from_secs_f64(duration)) => {}
                    _ = tokio::signal::ctrl_c() => {}
                }
            } else {
                tokio::signal::ctrl_c().await?;
            }
            println!("正在停止…");
            engine.stop_all();
        }
        Commands::Validate { profile } => {
            let text = std::fs::read_to_string(&profile)?;
            let _: Profile = serde_json::from_str(&text)?;
            println!("方案有效: {}", profile.display());
        }
        Commands::Schema => {
            let schema = schemars::schema_for!(Profile);
            println!("{}", serde_json::to_string_pretty(&schema)?);
        }
    }
    Ok(())
}
