mod commands;
mod data_gen;
pub mod engine;
pub mod events;
mod modbus_client;
mod modbus_slave;
pub mod models;
mod mqtt_client;
mod runtime;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let engine = commands::build_engine(app.handle().clone());
            app.manage(engine);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::load_profile,
            commands::load_profile_json,
            commands::save_profile,
            commands::dump_profile,
            commands::set_profile_meta,
            commands::add_instance,
            commands::remove_instance,
            commands::clone_instance,
            commands::update_instance,
            commands::start_instance,
            commands::stop_instance,
            commands::start_all,
            commands::stop_all,
            commands::trigger_once,
            commands::profile_schema,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DeviceSim");
}
