#![allow(non_snake_case, dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::fs_utils::atomic_write;
use tokio::{
    sync::{Mutex, RwLock},
    task::JoinHandle,
    time::{Duration, sleep},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeWorkerState {
    Starting,
    Running,
    Stopped,
    Failed,
}

impl RuntimeWorkerState {
    fn is_active(&self) -> bool {
        matches!(self, Self::Starting | Self::Running)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeWorkerStatus {
    pub worker: String,
    pub state: RuntimeWorkerState,
    pub active: bool,
    pub startedAtMs: Option<u128>,
    pub updatedAtMs: u128,
    pub lastHeartbeatMs: Option<u128>,
    pub startCount: u64,
    pub restartCount: u64,
    pub stopReason: Option<String>,
    pub lastError: Option<String>,
    pub lastAction: Option<String>,
    pub config: Option<Value>,
}

#[derive(Debug, Default)]
pub struct RuntimeRegistry {
    pub workers: RwLock<HashMap<String, RuntimeWorkerStatus>>,
    pub tasks: Mutex<HashMap<String, JoinHandle<()>>>,
    pub storage_path: PathBuf,
}

impl RuntimeRegistry {
    pub fn new(storage_path: PathBuf) -> Self {
        Self {
            workers: RwLock::new(HashMap::new()),
            tasks: Mutex::new(HashMap::new()),
            storage_path,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeRequest {
    pub worker: Option<String>,
    pub config: Option<Value>,
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeResponse {
    pub ok: bool,
    pub worker: Option<String>,
    pub active: Option<bool>,
    pub state: Option<RuntimeWorkerState>,
    pub workers: Vec<RuntimeWorkerStatus>,
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn config_restart_on_failure(config: &Option<Value>) -> bool {
    config
        .as_ref()
        .and_then(|value| value.get("restartOnFailure"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn config_max_restart_attempts(config: &Option<Value>) -> u64 {
    config
        .as_ref()
        .and_then(|value| value.get("maxRestartAttempts"))
        .and_then(Value::as_u64)
        .unwrap_or(3)
}

fn persist_workers_sync(storage_path: &PathBuf, workers: &[RuntimeWorkerStatus]) {
    if let Some(parent) = storage_path.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            eprintln!("Failed to create runtime storage directory: {error}");
            return;
        }
    }
    if let Ok(serialized) = serde_json::to_string_pretty(workers) {
        if let Err(error) = atomic_write(storage_path, serialized.as_bytes()) {
            eprintln!("Failed to persist runtime workers: {error}");
        }
    }
}

async fn persist_workers(registry: &RuntimeRegistry) {
    let workers = list_workers(registry).await;
    persist_workers_sync(&registry.storage_path, &workers);
}

async fn insert_status(registry: &RuntimeRegistry, status: RuntimeWorkerStatus) {
    registry
        .workers
        .write()
        .await
        .insert(status.worker.clone(), status);
    persist_workers(registry).await;
}

async fn set_running(registry: &RuntimeRegistry, worker: &str) {
    let now = now_ms();
    let worker_name = worker.trim().to_string();
    let previous = registry.workers.read().await.get(&worker_name).cloned();
    let status = RuntimeWorkerStatus {
        worker: worker_name.clone(),
        state: RuntimeWorkerState::Running,
        active: true,
        startedAtMs: previous
            .as_ref()
            .and_then(|entry| entry.startedAtMs)
            .or(Some(now)),
        updatedAtMs: now,
        lastHeartbeatMs: Some(now),
        startCount: previous.as_ref().map(|entry| entry.startCount).unwrap_or(1),
        restartCount: previous
            .as_ref()
            .map(|entry| entry.restartCount)
            .unwrap_or(0),
        stopReason: None,
        lastError: None,
        lastAction: Some("worker-running".to_string()),
        config: previous.and_then(|entry| entry.config),
    };
    insert_status(registry, status).await;
}

async fn set_heartbeat(registry: &RuntimeRegistry, worker: &str) {
    let now = now_ms();
    let worker_name = worker.trim().to_string();
    let previous = registry.workers.read().await.get(&worker_name).cloned();
    let status = RuntimeWorkerStatus {
        worker: worker_name.clone(),
        state: previous
            .as_ref()
            .map(|entry| entry.state.clone())
            .unwrap_or(RuntimeWorkerState::Running),
        active: previous
            .as_ref()
            .map(|entry| entry.state.is_active())
            .unwrap_or(true),
        startedAtMs: previous
            .as_ref()
            .and_then(|entry| entry.startedAtMs)
            .or(Some(now)),
        updatedAtMs: now,
        lastHeartbeatMs: Some(now),
        startCount: previous.as_ref().map(|entry| entry.startCount).unwrap_or(1),
        restartCount: previous
            .as_ref()
            .map(|entry| entry.restartCount)
            .unwrap_or(0),
        stopReason: previous.as_ref().and_then(|entry| entry.stopReason.clone()),
        lastError: previous.as_ref().and_then(|entry| entry.lastError.clone()),
        lastAction: Some("worker-heartbeat".to_string()),
        config: previous.and_then(|entry| entry.config),
    };
    insert_status(registry, status).await;
}

async fn abort_existing_task(registry: &RuntimeRegistry, worker: &str) {
    if let Some(handle) = registry.tasks.lock().await.remove(worker) {
        handle.abort();
    }
}

fn spawn_worker_loop(registry: Arc<RuntimeRegistry>, worker_name: String) -> JoinHandle<()> {
    tokio::spawn(async move {
        set_running(&registry, &worker_name).await;
        loop {
            sleep(Duration::from_secs(5)).await;
            set_heartbeat(&registry, &worker_name).await;
        }
    })
}

pub async fn start_worker(
    registry: &Arc<RuntimeRegistry>,
    worker: &str,
    config: Option<Value>,
) -> RuntimeResponse {
    let worker_name = worker.trim().to_string();
    let now = now_ms();
    let previous = registry.workers.read().await.get(&worker_name).cloned();
    abort_existing_task(registry, &worker_name).await;
    let status = RuntimeWorkerStatus {
        worker: worker_name.clone(),
        state: RuntimeWorkerState::Starting,
        active: RuntimeWorkerState::Starting.is_active(),
        startedAtMs: Some(now),
        updatedAtMs: now,
        lastHeartbeatMs: None,
        startCount: previous
            .as_ref()
            .map(|entry| entry.startCount + 1)
            .unwrap_or(1),
        restartCount: previous
            .as_ref()
            .map(|entry| entry.restartCount)
            .unwrap_or(0),
        stopReason: None,
        lastError: None,
        lastAction: Some("start".to_string()),
        config: config.or_else(|| previous.and_then(|entry| entry.config)),
    };
    insert_status(registry, status.clone()).await;
    let handle = spawn_worker_loop(Arc::clone(registry), worker_name.clone());
    registry
        .tasks
        .lock()
        .await
        .insert(worker_name.clone(), handle);
    RuntimeResponse {
        ok: true,
        worker: Some(worker_name),
        active: Some(status.active),
        state: Some(status.state.clone()),
        workers: list_workers(registry).await,
    }
}

pub async fn stop_worker(
    registry: &Arc<RuntimeRegistry>,
    worker: &str,
    reason: Option<String>,
) -> RuntimeResponse {
    let worker_name = worker.trim().to_string();
    let now = now_ms();
    let previous = registry.workers.read().await.get(&worker_name).cloned();
    abort_existing_task(registry, &worker_name).await;
    let status = RuntimeWorkerStatus {
        worker: worker_name.clone(),
        state: RuntimeWorkerState::Stopped,
        active: false,
        updatedAtMs: now,
        lastHeartbeatMs: previous.as_ref().and_then(|entry| entry.lastHeartbeatMs),
        startCount: previous.as_ref().map(|entry| entry.startCount).unwrap_or(0),
        restartCount: previous
            .as_ref()
            .map(|entry| entry.restartCount)
            .unwrap_or(0),
        startedAtMs: None,
        stopReason: reason.or_else(|| previous.as_ref().and_then(|entry| entry.stopReason.clone())),
        lastError: previous.as_ref().and_then(|entry| entry.lastError.clone()),
        lastAction: Some("stop".to_string()),
        config: previous.and_then(|entry| entry.config),
    };
    registry
        .workers
        .write()
        .await
        .insert(worker_name.clone(), status);
    persist_workers(registry).await;
    RuntimeResponse {
        ok: true,
        worker: Some(worker_name),
        active: Some(false),
        state: Some(RuntimeWorkerState::Stopped),
        workers: list_workers(registry).await,
    }
}

pub async fn heartbeat_worker(
    registry: &Arc<RuntimeRegistry>,
    worker: &str,
    note: Option<String>,
) -> RuntimeResponse {
    let worker_name = worker.trim().to_string();
    let now = now_ms();
    let mut guard = registry.workers.write().await;
    let previous = guard.get(&worker_name).cloned();
    let status = RuntimeWorkerStatus {
        worker: worker_name.clone(),
        state: previous
            .as_ref()
            .map(|entry| entry.state.clone())
            .unwrap_or(RuntimeWorkerState::Running),
        active: previous
            .as_ref()
            .map(|entry| entry.state.is_active())
            .unwrap_or(true),
        startedAtMs: previous
            .as_ref()
            .and_then(|entry| entry.startedAtMs)
            .or(Some(now)),
        updatedAtMs: now,
        lastHeartbeatMs: Some(now),
        startCount: previous.as_ref().map(|entry| entry.startCount).unwrap_or(1),
        restartCount: previous
            .as_ref()
            .map(|entry| entry.restartCount)
            .unwrap_or(0),
        stopReason: previous.as_ref().and_then(|entry| entry.stopReason.clone()),
        lastError: previous.as_ref().and_then(|entry| entry.lastError.clone()),
        lastAction: Some(note.unwrap_or_else(|| "heartbeat".to_string())),
        config: previous.and_then(|entry| entry.config),
    };
    guard.insert(worker_name.clone(), status.clone());
    drop(guard);
    persist_workers(registry).await;
    RuntimeResponse {
        ok: true,
        worker: Some(worker_name),
        active: Some(status.active),
        state: Some(status.state),
        workers: list_workers(registry).await,
    }
}

pub async fn fail_worker(
    registry: &Arc<RuntimeRegistry>,
    worker: &str,
    error: Option<String>,
) -> RuntimeResponse {
    let worker_name = worker.trim().to_string();
    let now = now_ms();
    let previous = registry.workers.read().await.get(&worker_name).cloned();
    abort_existing_task(registry, &worker_name).await;
    let next_config = previous.as_ref().and_then(|entry| entry.config.clone());
    let next_restart_count = previous
        .as_ref()
        .map(|entry| entry.restartCount)
        .unwrap_or(0);
    let should_restart = config_restart_on_failure(&next_config)
        && next_restart_count < config_max_restart_attempts(&next_config);
    let status = RuntimeWorkerStatus {
        worker: worker_name.clone(),
        state: if should_restart {
            RuntimeWorkerState::Starting
        } else {
            RuntimeWorkerState::Failed
        },
        active: should_restart,
        startedAtMs: previous.as_ref().and_then(|entry| entry.startedAtMs),
        updatedAtMs: now,
        lastHeartbeatMs: previous.as_ref().and_then(|entry| entry.lastHeartbeatMs),
        startCount: previous.as_ref().map(|entry| entry.startCount).unwrap_or(0),
        restartCount: if should_restart {
            next_restart_count + 1
        } else {
            next_restart_count
        },
        stopReason: previous.as_ref().and_then(|entry| entry.stopReason.clone()),
        lastError: error.or_else(|| previous.as_ref().and_then(|entry| entry.lastError.clone())),
        lastAction: Some(if should_restart {
            "auto-restart".to_string()
        } else {
            "fail".to_string()
        }),
        config: next_config.clone(),
    };
    insert_status(registry, status.clone()).await;
    if should_restart {
        let handle = spawn_worker_loop(Arc::clone(registry), worker_name.clone());
        registry
            .tasks
            .lock()
            .await
            .insert(worker_name.clone(), handle);
    }
    RuntimeResponse {
        ok: true,
        worker: Some(worker_name),
        active: Some(status.active),
        state: Some(status.state),
        workers: list_workers(registry).await,
    }
}

pub async fn list_workers(registry: &RuntimeRegistry) -> Vec<RuntimeWorkerStatus> {
    let mut workers: Vec<_> = registry.workers.read().await.values().cloned().collect();
    workers.sort_by(|left, right| left.worker.cmp(&right.worker));
    workers
}

pub async fn restore_workers(registry: &Arc<RuntimeRegistry>) -> Vec<RuntimeWorkerStatus> {
    let raw = match fs::read_to_string(&registry.storage_path) {
        Ok(raw) => raw,
        Err(_) => return vec![],
    };
    let restored: Vec<RuntimeWorkerStatus> = serde_json::from_str(&raw).unwrap_or_default();
    if restored.is_empty() {
        return restored;
    }

    {
        let mut guard = registry.workers.write().await;
        for worker in &restored {
            guard.insert(worker.worker.clone(), worker.clone());
        }
    }

    for worker in &restored {
        if worker.active
            || matches!(
                worker.state,
                RuntimeWorkerState::Starting | RuntimeWorkerState::Running
            )
        {
            let handle = spawn_worker_loop(Arc::clone(registry), worker.worker.clone());
            registry
                .tasks
                .lock()
                .await
                .insert(worker.worker.clone(), handle);
        }
    }

    persist_workers(registry).await;
    list_workers(registry).await
}
