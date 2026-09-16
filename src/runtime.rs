//! Process-wide tokio runtime and shared MoQ clients.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use anyhow::{Context, Result};
use moq_native::{Client, ClientConfig};
use tokio::runtime::Runtime;

/// Overrides the worker thread count of the shared runtime.
pub const WORKER_THREADS_ENV: &str = "MOQ_WORKER_THREADS";

const DEFAULT_MAX_WORKER_THREADS: usize = 4;

static RUNTIME: OnceLock<std::result::Result<Runtime, String>> = OnceLock::new();
static CLIENTS: OnceLock<Mutex<HashMap<String, Client>>> = OnceLock::new();

fn worker_threads() -> usize {
    std::env::var(WORKER_THREADS_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(1)
                .min(DEFAULT_MAX_WORKER_THREADS)
        })
}

/// The runtime shared by every session in the process, built on first use.
///
/// Worker count defaults to `min(cores, 4)` and can be set with `MOQ_WORKER_THREADS`
/// before the first session is created.
pub fn shared_runtime() -> Result<&'static Runtime> {
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(worker_threads())
                .thread_name("moq-worker")
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|e| anyhow::anyhow!("Failed to build MoQ runtime: {}", e))
}

/// Returns a client for `config`, creating it once per distinct config.
///
/// The client's UDP socket and endpoint driver live on the shared runtime so they
/// outlive whichever runtime first asked for them.
pub fn shared_client(config: &ClientConfig) -> Result<Client> {
    let key = serde_json::to_string(config).context("Failed to serialize client config")?;

    let clients = CLIENTS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut clients = clients.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(client) = clients.get(&key) {
        return Ok(client.clone());
    }

    let _guard = shared_runtime()?.enter();
    let client = config
        .clone()
        .init()
        .context("Failed to initialize MoQ client")?;
    clients.insert(key, client.clone());
    Ok(client)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cached_clients() -> usize {
        CLIENTS
            .get()
            .map(|c| c.lock().unwrap_or_else(|e| e.into_inner()).len())
            .unwrap_or(0)
    }

    #[test]
    fn shared_runtime_is_bounded_and_reused() {
        let a = shared_runtime().unwrap();
        let b = shared_runtime().unwrap();
        assert!(std::ptr::eq(a, b));
        assert!(a.metrics().num_workers() <= worker_threads());
    }

    #[test]
    fn shared_client_is_created_once_per_config() {
        let mut config = ClientConfig::default();
        config.bind = "127.0.0.1:0".parse().unwrap();

        shared_client(&config).unwrap();
        let after_first = cached_clients();
        shared_client(&config).unwrap();
        assert_eq!(cached_clients(), after_first);

        let mut other = config.clone();
        other.timeout = Some(std::time::Duration::from_secs(7));
        shared_client(&other).unwrap();
        assert_eq!(cached_clients(), after_first + 1);
    }
}
