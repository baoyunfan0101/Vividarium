use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use chrono::Local;
use phytoindex_core::{Database, OperationState, OperationsStatus};
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;

static GLOBAL_STATE: OnceLock<AppState> = OnceLock::new();
const PROGRESS_EVENT_INTERVAL: Duration = Duration::from_millis(100);

struct ProgressEventThrottle {
    last_emitted_at: Option<Instant>,
    last_message: Option<String>,
    last_processed: Option<u64>,
    last_total: Option<u64>,
}

impl ProgressEventThrottle {
    fn new() -> Self {
        Self {
            last_emitted_at: None,
            last_message: None,
            last_processed: None,
            last_total: None,
        }
    }

    fn should_emit(&mut self, processed: u64, total: Option<u64>, message: &str) -> bool {
        let first = self.last_emitted_at.is_none();
        let phase_changed =
            self.last_message.as_deref() != Some(message) || self.last_total != total;
        let completed =
            total.is_some_and(|value| processed >= value) && self.last_processed != Some(processed);
        let interval_elapsed = self
            .last_emitted_at
            .is_some_and(|instant| instant.elapsed() >= PROGRESS_EVENT_INTERVAL);
        let emit = first || phase_changed || completed || interval_elapsed;
        if emit {
            self.last_emitted_at = Some(Instant::now());
            self.last_message = Some(message.into());
            self.last_processed = Some(processed);
            self.last_total = total;
        }
        emit
    }
}

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    pub thumbnail_dir: PathBuf,
    pub operations: OperationManager,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Result<Self, phytoindex_core::CoreError> {
        let database = Database::open(data_dir.join("phytoindex.db"))?;
        let thumbnail_dir = data_dir.join("thumbnails");
        phytoindex_core::photos::rebase_thumbnail_paths(&database, &thumbnail_dir)?;
        Ok(Self {
            database,
            thumbnail_dir,
            operations: OperationManager::new(),
        })
    }
}

pub fn set_global(state: AppState) -> Result<(), AppState> {
    GLOBAL_STATE.set(state)
}

pub fn global() -> Option<&'static AppState> {
    GLOBAL_STATE.get()
}

#[derive(Clone)]
pub struct OperationManager {
    states: Arc<Mutex<OperationsStatus>>,
}

impl OperationManager {
    fn new() -> Self {
        let states = ["photos", "taxa", "mapping"]
            .into_iter()
            .map(|module| (module.to_string(), idle_state(module)))
            .collect();
        Self {
            states: Arc::new(Mutex::new(states)),
        }
    }

    pub fn status(&self) -> OperationsStatus {
        self.states
            .lock()
            .expect("operation state lock poisoned")
            .clone()
    }

    pub fn start<F>(
        &self,
        app: AppHandle,
        module: &'static str,
        operation: &'static str,
        callback: F,
    ) -> Result<OperationState, String>
    where
        F: FnOnce(&mut (dyn FnMut(u64, Option<u64>, &str) + Send)) -> Result<Value, String>
            + Send
            + 'static,
    {
        let task_id = Uuid::new_v4().simple().to_string();
        let state = {
            let mut states = self.states.lock().map_err(|error| error.to_string())?;
            if let Some(blocked_by) = blocked_by(&states, module) {
                return Err(format!("{module} is blocked by {blocked_by}"));
            }
            let state = OperationState {
                module: module.into(),
                task_id: Some(task_id.clone()),
                operation: Some(operation.into()),
                running: true,
                started_at: Some(now()),
                finished_at: None,
                message: format!("{operation} running"),
                processed: 0,
                total: None,
                result: None,
                error: None,
            };
            states.insert(module.into(), state.clone());
            state
        };
        let manager = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let progress_manager = manager.clone();
            let progress_app = app.clone();
            let progress_task_id = task_id.clone();
            let mut throttle = ProgressEventThrottle::new();
            let mut progress = move |processed: u64, total: Option<u64>, message: &str| {
                let emit = throttle.should_emit(processed, total, message);
                let current = progress_manager.update_progress(
                    module,
                    &progress_task_id,
                    processed,
                    total,
                    message,
                    emit,
                );
                if let Some(current) = current {
                    let _ = progress_app.emit("operation-progress", current);
                }
            };
            let result = callback(&mut progress);
            let finished = manager.finish(module, &task_id, result);
            if let Some(finished) = finished {
                let _ = app.emit("operation-progress", finished);
            }
        });
        Ok(state)
    }

    fn update_progress(
        &self,
        module: &str,
        task_id: &str,
        processed: u64,
        total: Option<u64>,
        message: &str,
        snapshot: bool,
    ) -> Option<OperationState> {
        let mut states = self.states.lock().ok()?;
        let state = states.get_mut(module)?;
        if state.task_id.as_deref() != Some(task_id) || !state.running {
            return None;
        }
        state.processed = processed;
        state.total = total;
        state.message = message.into();
        snapshot.then(|| state.clone())
    }

    fn finish(
        &self,
        module: &str,
        task_id: &str,
        result: Result<Value, String>,
    ) -> Option<OperationState> {
        let mut states = self.states.lock().ok()?;
        let state = states.get_mut(module)?;
        if state.task_id.as_deref() != Some(task_id) {
            return None;
        }
        state.running = false;
        state.finished_at = Some(now());
        match result {
            Ok(result) => {
                state.message = "completed".into();
                state.result = Some(result);
                state.error = None;
            }
            Err(error) => {
                state.message = "failed".into();
                state.error = Some(error);
            }
        }
        Some(state.clone())
    }
}

fn idle_state(module: &str) -> OperationState {
    OperationState {
        module: module.into(),
        task_id: None,
        operation: None,
        running: false,
        started_at: None,
        finished_at: None,
        message: "idle".into(),
        processed: 0,
        total: None,
        result: None,
        error: None,
    }
}

fn blocked_by(states: &BTreeMap<String, OperationState>, module: &str) -> Option<String> {
    states.iter().find_map(|(other, state)| {
        (state.running && (module == other || module == "mapping" || other == "mapping"))
            .then(|| other.clone())
    })
}

fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_throttle_emits_first_phase_changes_and_completion() {
        let mut throttle = ProgressEventThrottle::new();

        assert!(throttle.should_emit(0, None, "Reading"));
        assert!(!throttle.should_emit(1, None, "Reading"));
        assert!(throttle.should_emit(0, Some(100), "Importing"));
        assert!(!throttle.should_emit(1, Some(100), "Importing"));
        assert!(throttle.should_emit(100, Some(100), "Importing"));
        assert!(throttle.should_emit(100, None, "Committing"));
    }
}
