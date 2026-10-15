use std::sync::OnceLock;

use tokio::runtime::{Handle, Runtime};
use tokio::task::JoinHandle;

fn dedicated_runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .thread_name("devicesim")
            .build()
            .expect("failed to create tokio runtime")
    })
}

/// Spawn onto the current Tokio runtime if available (CLI), otherwise a dedicated runtime (Tauri sync commands).
pub fn spawn<F>(future: F) -> JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send + 'static,
{
    match Handle::try_current() {
        Ok(handle) => handle.spawn(future),
        Err(_) => dedicated_runtime().spawn(future),
    }
}
