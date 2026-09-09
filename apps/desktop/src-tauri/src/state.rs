use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use chrono::Local;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use uuid::Uuid;
use vividarium_core::taxonomy::{PreparedTaxonomyUpdate, TaxonomyPreviewResult};
use vividarium_core::{
    BackgroundTaskState, CancellationToken, CoreError, Database, OperationProgress,
    OperationProgressUnit, OperationState, OperationsStatus,
};

static GLOBAL_STATE: OnceLock<AppState> = OnceLock::new();
const PROGRESS_EVENT_INTERVAL: Duration = Duration::from_millis(100);

struct ProgressEventThrottle {
    last_emitted_at: Option<Instant>,
    last_stage: Option<String>,
    last_current: Option<u64>,
    last_total: Option<u64>,
    last_unit: Option<OperationProgressUnit>,
}

impl ProgressEventThrottle {
    fn new() -> Self {
        Self {
            last_emitted_at: None,
            last_stage: None,
            last_current: None,
            last_total: None,
            last_unit: None,
        }
    }

    fn should_emit(&mut self, progress: &OperationProgress) -> bool {
        let first = self.last_emitted_at.is_none();
        let phase_changed = self.last_stage.as_deref() != Some(&progress.stage)
            || self.last_total != progress.total
            || self.last_unit != progress.unit;
        let completed = progress
            .total
            .zip(progress.current)
            .is_some_and(|(total, current)| current >= total)
            && self.last_current != progress.current;
        let interval_elapsed = self
            .last_emitted_at
            .is_some_and(|instant| instant.elapsed() >= PROGRESS_EVENT_INTERVAL);
        let emit = first || phase_changed || completed || interval_elapsed;
        if emit {
            self.last_emitted_at = Some(Instant::now());
            self.last_stage = Some(progress.stage.clone());
            self.last_current = progress.current;
            self.last_total = progress.total;
            self.last_unit = progress.unit;
        }
        emit
    }
}

#[derive(Clone)]
pub struct AppState {
    pub database: Database,
    pub thumbnail_dir: PathBuf,
    pub operations: OperationManager,
    pub background_tasks: BackgroundTaskScheduler,
    pub active_tasks: ActiveTaskRegistry,
    photo_library_lifecycle: Arc<Mutex<()>>,
    formatted_update_preview: Arc<Mutex<Option<StagedFormattedUpdate>>>,
}

#[derive(Debug)]
struct StagedFormattedUpdate {
    owner_id: String,
    preview_id: String,
    prepared: PreparedTaxonomyUpdate,
}

impl AppState {
    pub fn new(data_dir: PathBuf) -> Result<Self, vividarium_core::CoreError> {
        let database = Database::open(data_dir.join("metadata.db"))?;
        let thumbnail_dir = data_dir.join("thumbnails");
        if database.active_photo_library()?.is_some() {
            let _ = vividarium_core::photos::rebase_thumbnail_paths(&database, &thumbnail_dir);
        }
        let operations = OperationManager::new();
        Ok(Self {
            database,
            thumbnail_dir,
            background_tasks: BackgroundTaskScheduler::new(operations.clone()),
            operations,
            active_tasks: ActiveTaskRegistry::default(),
            photo_library_lifecycle: Arc::new(Mutex::new(())),
            formatted_update_preview: Arc::new(Mutex::new(None)),
        })
    }

    pub fn lock_photo_library_lifecycle(&self) -> Result<MutexGuard<'_, ()>, String> {
        self.photo_library_lifecycle
            .lock()
            .map_err(|_| "photo library lifecycle lock is poisoned".to_string())
    }

    pub fn replace_formatted_update_preview(
        &self,
        owner_id: String,
        prepared: PreparedTaxonomyUpdate,
    ) -> Result<(String, TaxonomyPreviewResult), CoreError> {
        let preview_id = Uuid::new_v4().to_string();
        let preview = prepared.preview_result().clone();
        let mut current = self.formatted_update_preview.lock().map_err(|_| {
            CoreError::Consistency("formatted update preview lock is poisoned".into())
        })?;
        *current = Some(StagedFormattedUpdate {
            owner_id,
            preview_id: preview_id.clone(),
            prepared,
        });
        Ok((preview_id, preview))
    }

    pub fn take_formatted_update_preview(
        &self,
        owner_id: &str,
        preview_id: &str,
    ) -> Result<PreparedTaxonomyUpdate, CoreError> {
        let mut current = self.formatted_update_preview.lock().map_err(|_| {
            CoreError::Consistency("formatted update preview lock is poisoned".into())
        })?;
        if current
            .as_ref()
            .map(|value| (value.owner_id.as_str(), value.preview_id.as_str()))
            != Some((owner_id, preview_id))
        {
            return Err(CoreError::InvalidArgument(
                "formatted update preview is no longer current; preview again".into(),
            ));
        }
        current
            .take()
            .map(|value| value.prepared)
            .ok_or_else(|| CoreError::Consistency("formatted update preview disappeared".into()))
    }

    pub fn clear_formatted_update_preview(&self, owner_id: &str) -> Result<(), CoreError> {
        let mut current = self.formatted_update_preview.lock().map_err(|_| {
            CoreError::Consistency("formatted update preview lock is poisoned".into())
        })?;
        if current
            .as_ref()
            .is_some_and(|value| value.owner_id == owner_id)
        {
            *current = None;
        }
        Ok(())
    }

    #[cfg(test)]
    pub fn has_formatted_update_preview(&self, owner_id: &str) -> bool {
        self.formatted_update_preview
            .lock()
            .ok()
            .and_then(|current| current.as_ref().map(|value| value.owner_id == owner_id))
            .unwrap_or(false)
    }
}

#[derive(Clone, Default)]
pub struct ActiveTaskRegistry {
    state: Arc<Mutex<ActiveTaskRegistryState>>,
}

#[derive(Default)]
struct ActiveTaskRegistryState {
    tasks: HashMap<String, HashMap<String, CancellationToken>>,
    cancelled_owners: HashSet<String>,
}

impl ActiveTaskRegistry {
    pub fn start(&self, owner_id: String) -> Result<ActiveTask, String> {
        let task_id = Uuid::new_v4().simple().to_string();
        let cancellation = CancellationToken::new();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "active task registry lock is poisoned".to_string())?;
        if state.cancelled_owners.contains(&owner_id) {
            return Err("operation cancelled".into());
        }
        state
            .tasks
            .entry(owner_id.clone())
            .or_default()
            .insert(task_id.clone(), cancellation.clone());
        drop(state);
        Ok(ActiveTask {
            registry: self.clone(),
            owner_id,
            task_id,
            cancellation,
        })
    }

    pub fn cancel_owner(&self, owner_id: &str) -> Result<usize, String> {
        let cancellations = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| "active task registry lock is poisoned".to_string())?;
            state.cancelled_owners.insert(owner_id.to_string());
            state
                .tasks
                .remove(owner_id)
                .map(|tasks| tasks.into_values().collect::<Vec<_>>())
                .unwrap_or_default()
        };
        for cancellation in &cancellations {
            cancellation.cancel();
        }
        Ok(cancellations.len())
    }

    fn finish(&self, owner_id: &str, task_id: &str) {
        let Ok(mut state) = self.state.lock() else {
            return;
        };
        let Some(owner_tasks) = state.tasks.get_mut(owner_id) else {
            return;
        };
        owner_tasks.remove(task_id);
        if owner_tasks.is_empty() {
            state.tasks.remove(owner_id);
        }
    }
}

pub struct ActiveTask {
    registry: ActiveTaskRegistry,
    owner_id: String,
    task_id: String,
    cancellation: CancellationToken,
}

impl ActiveTask {
    pub fn cancellation(&self) -> CancellationToken {
        self.cancellation.clone()
    }
}

impl Drop for ActiveTask {
    fn drop(&mut self) {
        self.registry.finish(&self.owner_id, &self.task_id);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BackgroundTaskKind {
    PhotoScan,
    MetadataIndex,
    PhotoMapping,
}

impl BackgroundTaskKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::PhotoScan => "photo_scan",
            Self::MetadataIndex => "metadata_index",
            Self::PhotoMapping => "photo_mapping",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackgroundTaskKey {
    pub kind: BackgroundTaskKind,
    pub scope: String,
}

impl BackgroundTaskKey {
    pub fn new(kind: BackgroundTaskKind, scope: impl Into<String>) -> Self {
        Self {
            kind,
            scope: scope.into(),
        }
    }
}

type BackgroundCallback = Box<
    dyn FnOnce(&mut (dyn FnMut(OperationProgress) + Send)) -> Result<Value, String>
        + Send
        + 'static,
>;

struct PendingBackgroundWork {
    key: BackgroundTaskKey,
    module: &'static str,
    operation: &'static str,
    callback: BackgroundCallback,
}

struct ScheduledBackgroundWork {
    task_id: String,
    pending: PendingBackgroundWork,
}

#[derive(Default)]
struct BackgroundQueue {
    queued: VecDeque<ScheduledBackgroundWork>,
    task_ids: HashMap<BackgroundTaskKey, String>,
    reruns: HashMap<BackgroundTaskKey, PendingBackgroundWork>,
}

impl BackgroundQueue {
    fn push(&mut self, work: ScheduledBackgroundWork) -> Result<(), String> {
        if let Some(existing) = self.task_ids.get(&work.pending.key) {
            return Err(existing.clone());
        }
        self.task_ids
            .insert(work.pending.key.clone(), work.task_id.clone());
        self.queued.push_back(work);
        Ok(())
    }

    fn complete(&mut self, key: &BackgroundTaskKey) -> Option<PendingBackgroundWork> {
        self.task_ids.remove(key);
        self.reruns.remove(key)
    }
}

#[derive(Clone)]
pub struct BackgroundTaskScheduler {
    queue: Arc<Mutex<BackgroundQueue>>,
    worker_active: Arc<AtomicBool>,
    operations: OperationManager,
}

impl BackgroundTaskScheduler {
    fn new(operations: OperationManager) -> Self {
        Self {
            queue: Arc::new(Mutex::new(BackgroundQueue::default())),
            worker_active: Arc::new(AtomicBool::new(false)),
            operations,
        }
    }

    pub fn enqueue<F>(
        &self,
        app: AppHandle,
        key: BackgroundTaskKey,
        module: &'static str,
        operation: &'static str,
        rerun_if_running: bool,
        callback: F,
    ) -> Result<OperationState, String>
    where
        F: FnOnce(&mut (dyn FnMut(OperationProgress) + Send)) -> Result<Value, String>
            + Send
            + 'static,
    {
        let pending = PendingBackgroundWork {
            key: key.clone(),
            module,
            operation,
            callback: Box::new(callback),
        };
        let state = {
            let mut queue = self.queue.lock().map_err(|error| error.to_string())?;
            if let Some(task_id) = queue.task_ids.get(&key).cloned() {
                let existing = self
                    .operations
                    .operation(&task_id)
                    .ok_or_else(|| format!("background task registry lost task {task_id}"))?;
                if existing.is_active() {
                    if rerun_if_running && existing.state == BackgroundTaskState::Running {
                        queue.reruns.insert(key, pending);
                    }
                    return Ok(existing);
                }
                queue.reruns.insert(key, pending);
                return Ok(existing);
            }
            let state = self.operations.queue_background(module, operation, &key)?;
            let task_id = state
                .task_id
                .clone()
                .ok_or_else(|| "queued background task has no task id".to_string())?;
            queue.push(ScheduledBackgroundWork { task_id, pending })?;
            state
        };
        let _ = app.emit("operation-progress", state.clone());
        self.start_worker(app);
        Ok(state)
    }

    fn start_worker(&self, app: AppHandle) {
        if self
            .worker_active
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return;
        }
        let scheduler = self.clone();
        tauri::async_runtime::spawn_blocking(move || scheduler.run(app));
    }

    fn run(&self, app: AppHandle) {
        loop {
            let work = self
                .queue
                .lock()
                .ok()
                .and_then(|mut queue| queue.queued.pop_front());
            let Some(work) = work else {
                self.worker_active.store(false, Ordering::Release);
                let has_more = self
                    .queue
                    .lock()
                    .is_ok_and(|queue| !queue.queued.is_empty());
                if has_more
                    && self
                        .worker_active
                        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                        .is_ok()
                {
                    continue;
                }
                break;
            };
            while self
                .operations
                .blocked_by_running_other(work.pending.module, &work.task_id)
                .is_some()
            {
                std::thread::sleep(Duration::from_millis(25));
            }
            self.operations
                .run_queued(app.clone(), &work.task_id, work.pending.callback);
            let next = {
                let mut queue = match self.queue.lock() {
                    Ok(queue) => queue,
                    Err(_) => break,
                };
                queue.complete(&work.pending.key)
            };
            if let Some(pending) = next {
                let key = pending.key.clone();
                let state =
                    match self
                        .operations
                        .queue_background(pending.module, pending.operation, &key)
                    {
                        Ok(state) => state,
                        Err(_) => continue,
                    };
                let Some(task_id) = state.task_id.clone() else {
                    continue;
                };
                if let Ok(mut queue) = self.queue.lock() {
                    let _ = queue.push(ScheduledBackgroundWork { task_id, pending });
                }
                let _ = app.emit("operation-progress", state);
            }
        }
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
        Self {
            states: Arc::new(Mutex::new(OperationsStatus::new())),
        }
    }

    pub fn status(&self) -> OperationsStatus {
        self.states
            .lock()
            .expect("operation state lock poisoned")
            .clone()
    }

    #[cfg(test)]
    pub fn start_with_progress_for_test<F>(
        &self,
        module: &'static str,
        operation: &'static str,
        callback: F,
    ) -> Result<(OperationState, std::sync::mpsc::Receiver<OperationState>), String>
    where
        F: FnOnce(&mut (dyn FnMut(OperationProgress) + Send)) -> Result<Value, String>
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
                task_kind: None,
                task_scope: None,
                state: BackgroundTaskState::Running,
                operation: Some(operation.into()),
                started_at: Some(now()),
                finished_at: None,
                progress: None,
                result: None,
                error: None,
            };
            states.insert(task_id.clone(), state.clone());
            state
        };
        let (terminal_sender, terminal_receiver) = std::sync::mpsc::channel();
        let manager = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let progress_manager = manager.clone();
            let progress_task_id = task_id.clone();
            let mut progress = move |progress: OperationProgress| {
                let _ = progress_manager.update_progress(&progress_task_id, &progress, false);
            };
            let result = callback(&mut progress);
            if let Some(finished) = manager.finish(&task_id, result) {
                let _ = terminal_sender.send(finished);
            }
        });
        Ok((state, terminal_receiver))
    }

    pub fn start_with_progress<F>(
        &self,
        app: AppHandle,
        module: &'static str,
        operation: &'static str,
        callback: F,
    ) -> Result<OperationState, String>
    where
        F: FnOnce(&mut (dyn FnMut(OperationProgress) + Send)) -> Result<Value, String>
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
                task_kind: None,
                task_scope: None,
                state: BackgroundTaskState::Running,
                operation: Some(operation.into()),
                started_at: Some(now()),
                finished_at: None,
                progress: None,
                result: None,
                error: None,
            };
            states.insert(task_id.clone(), state.clone());
            state
        };
        let manager = self.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let progress_manager = manager.clone();
            let progress_app = app.clone();
            let progress_task_id = task_id.clone();
            let mut throttle = ProgressEventThrottle::new();
            let mut progress = move |progress: OperationProgress| {
                let emit = throttle.should_emit(&progress);
                let current = progress_manager.update_progress(&progress_task_id, &progress, emit);
                if let Some(current) = current {
                    let _ = progress_app.emit("operation-progress", current);
                }
            };
            let result = callback(&mut progress);
            let finished = manager.finish(&task_id, result);
            if let Some(finished) = finished {
                let _ = app.emit("operation-progress", finished);
            }
        });
        Ok(state)
    }

    fn update_progress(
        &self,
        task_id: &str,
        progress: &OperationProgress,
        snapshot: bool,
    ) -> Option<OperationState> {
        let mut states = self.states.lock().ok()?;
        let state = states.get_mut(task_id)?;
        if state.state != BackgroundTaskState::Running {
            return None;
        }
        state.progress = Some(progress.clone());
        snapshot.then(|| state.clone())
    }

    fn finish(&self, task_id: &str, result: Result<Value, String>) -> Option<OperationState> {
        let mut states = self.states.lock().ok()?;
        let state = states.get_mut(task_id)?;
        state.finished_at = Some(now());
        match result {
            Ok(result) => {
                state.state = BackgroundTaskState::Completed;
                state.result = Some(result);
                state.error = None;
            }
            Err(error) => {
                state.state = BackgroundTaskState::Failed;
                state.error = Some(error);
            }
        }
        let finished = state.clone();
        trim_finished_operations(&mut states, 50);
        Some(finished)
    }

    fn operation(&self, task_id: &str) -> Option<OperationState> {
        self.states.lock().ok()?.get(task_id).cloned()
    }

    fn queue_background(
        &self,
        module: &'static str,
        operation: &'static str,
        key: &BackgroundTaskKey,
    ) -> Result<OperationState, String> {
        let task_id = Uuid::new_v4().simple().to_string();
        let state = OperationState {
            module: module.into(),
            task_id: Some(task_id.clone()),
            task_kind: Some(key.kind.as_str().into()),
            task_scope: Some(key.scope.clone()),
            state: BackgroundTaskState::Queued,
            operation: Some(operation.into()),
            started_at: None,
            finished_at: None,
            progress: None,
            result: None,
            error: None,
        };
        self.states
            .lock()
            .map_err(|error| error.to_string())?
            .insert(task_id, state.clone());
        Ok(state)
    }

    fn blocked_by_running_other(&self, module: &str, task_id: &str) -> Option<String> {
        let states = self.states.lock().ok()?;
        blocked_by_running_excluding(&states, module, Some(task_id))
    }

    fn run_queued(&self, app: AppHandle, task_id: &str, callback: BackgroundCallback) {
        let Some(running) = self.mark_running(task_id) else {
            return;
        };
        let _ = app.emit("operation-progress", running);
        let progress_manager = self.clone();
        let progress_app = app.clone();
        let progress_task_id = task_id.to_string();
        let mut throttle = ProgressEventThrottle::new();
        let mut progress = move |value: OperationProgress| {
            let emit = throttle.should_emit(&value);
            let current = progress_manager.update_progress(&progress_task_id, &value, emit);
            if let Some(current) = current {
                let _ = progress_app.emit("operation-progress", current);
            }
        };
        let result = callback(&mut progress);
        if let Some(finished) = self.finish(task_id, result) {
            let _ = app.emit("operation-progress", finished);
        }
    }

    fn mark_running(&self, task_id: &str) -> Option<OperationState> {
        let mut states = self.states.lock().ok()?;
        let state = states.get_mut(task_id)?;
        state.state = BackgroundTaskState::Running;
        state.started_at = Some(now());
        Some(state.clone())
    }
}

fn trim_finished_operations(states: &mut OperationsStatus, limit: usize) {
    let mut finished = states
        .iter()
        .filter(|(_, state)| {
            matches!(
                state.state,
                BackgroundTaskState::Completed | BackgroundTaskState::Failed
            )
        })
        .map(|(task_id, state)| (task_id.clone(), state.finished_at.clone()))
        .collect::<Vec<_>>();
    if finished.len() <= limit {
        return;
    }
    finished.sort_by(|left, right| left.1.cmp(&right.1));
    let remove_count = finished.len() - limit;
    for (task_id, _) in finished.into_iter().take(remove_count) {
        states.remove(&task_id);
    }
}

fn blocked_by(states: &BTreeMap<String, OperationState>, module: &str) -> Option<String> {
    blocked_by_excluding(states, module, None)
}

fn blocked_by_excluding(
    states: &BTreeMap<String, OperationState>,
    module: &str,
    excluded_task_id: Option<&str>,
) -> Option<String> {
    states.values().find_map(|state| {
        if state.task_id.as_deref() == excluded_task_id {
            return None;
        }
        (state.is_active() && modules_conflict(module, &state.module)).then(|| state.module.clone())
    })
}

fn blocked_by_running_excluding(
    states: &BTreeMap<String, OperationState>,
    module: &str,
    excluded_task_id: Option<&str>,
) -> Option<String> {
    states.values().find_map(|state| {
        if state.task_id.as_deref() == excluded_task_id
            || state.state != BackgroundTaskState::Running
        {
            return None;
        }
        modules_conflict(module, &state.module).then(|| state.module.clone())
    })
}

fn modules_conflict(module: &str, other: &str) -> bool {
    let taxonomy_import_conflict = matches!(module, "sql_import" | "direct_import")
        && matches!(other, "sql_import" | "direct_import");
    module == other || module == "mapping" || other == "mapping" || taxonomy_import_conflict
}

fn now() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn progress_throttle_emits_first_phase_changes_and_completion() {
        let mut throttle = ProgressEventThrottle::new();

        let progress = |stage: &str, current, total, unit| OperationProgress {
            stage: stage.into(),
            current,
            total,
            unit,
        };
        assert!(throttle.should_emit(&progress("reading", None, None, None)));
        assert!(!throttle.should_emit(&progress("reading", Some(1), None, None)));
        assert!(throttle.should_emit(&progress(
            "importing",
            Some(0),
            Some(100),
            Some(OperationProgressUnit::Items),
        )));
        assert!(!throttle.should_emit(&progress(
            "importing",
            Some(1),
            Some(100),
            Some(OperationProgressUnit::Items),
        )));
        assert!(throttle.should_emit(&progress(
            "importing",
            Some(100),
            Some(100),
            Some(OperationProgressUnit::Items),
        )));
        assert!(throttle.should_emit(&progress("committing", None, None, None)));
    }

    #[test]
    fn active_tasks_are_cancelled_by_owner_only() {
        let registry = ActiveTaskRegistry::default();
        let first = registry.start("tab-a".into()).unwrap();
        let second = registry.start("tab-b".into()).unwrap();
        let first_cancellation = first.cancellation();
        let second_cancellation = second.cancellation();

        assert_eq!(registry.cancel_owner("tab-a").unwrap(), 1);
        assert!(first_cancellation.is_cancelled());
        assert!(!second_cancellation.is_cancelled());
        assert_eq!(registry.cancel_owner("tab-a").unwrap(), 0);
        assert_eq!(
            registry.start("tab-a".into()).err().as_deref(),
            Some("operation cancelled")
        );
    }

    #[test]
    fn structured_progress_is_retained_in_operation_status() {
        let manager = OperationManager::new();
        {
            let mut states = manager.states.lock().unwrap();
            states.insert(
                "validate-1".into(),
                OperationState {
                    module: "sql_import".into(),
                    task_id: Some("validate-1".into()),
                    task_kind: None,
                    task_scope: None,
                    state: BackgroundTaskState::Running,
                    operation: Some("validate_sql_import".into()),
                    started_at: Some(now()),
                    finished_at: None,
                    progress: None,
                    result: None,
                    error: None,
                },
            );
        }
        let progress = OperationProgress {
            stage: "executing_sql".into(),
            current: Some(2),
            total: Some(7),
            unit: Some(OperationProgressUnit::Statements),
        };

        let state = manager
            .update_progress("validate-1", &progress, true)
            .unwrap();

        assert_eq!(state.progress, Some(progress));
    }

    #[test]
    fn progress_serializes_determinate_and_indeterminate_units() {
        let determinate = OperationProgress {
            stage: "normalizing_names".into(),
            current: Some(12),
            total: Some(20),
            unit: Some(OperationProgressUnit::Names),
        };
        assert_eq!(
            serde_json::to_value(&determinate).unwrap(),
            serde_json::json!({
                "stage": "normalizing_names",
                "current": 12,
                "total": 20,
                "unit": "names",
            })
        );
        let indeterminate = OperationProgress {
            stage: "checking_database".into(),
            current: None,
            total: None,
            unit: None,
        };
        assert_eq!(
            serde_json::to_value(&indeterminate).unwrap(),
            serde_json::json!({
                "stage": "checking_database",
                "current": null,
                "total": null,
                "unit": null,
            })
        );
    }

    #[test]
    fn duplicate_background_task_keys_are_coalesced() {
        let key = BackgroundTaskKey::new(BackgroundTaskKind::MetadataIndex, "library-a");
        let mut queue = BackgroundQueue::default();
        queue.push(scheduled_work("task-1", key.clone())).unwrap();

        assert_eq!(
            queue.push(scheduled_work("task-2", key)).unwrap_err(),
            "task-1"
        );
        assert_eq!(queue.queued.len(), 1);
    }

    #[test]
    fn background_task_state_moves_from_queued_to_running_to_completed() {
        let manager = OperationManager::new();
        let key = BackgroundTaskKey::new(BackgroundTaskKind::PhotoScan, "library-a");
        let queued = manager
            .queue_background("photos", "photo_scan", &key)
            .unwrap();
        let task_id = queued.task_id.as_deref().unwrap();
        assert_eq!(queued.state, BackgroundTaskState::Queued);

        let running = manager.mark_running(task_id).unwrap();
        assert_eq!(running.state, BackgroundTaskState::Running);

        let completed = manager.finish(task_id, Ok(Value::Null)).unwrap();
        assert_eq!(completed.state, BackgroundTaskState::Completed);
    }

    #[test]
    fn background_task_failure_retains_the_error() {
        let manager = OperationManager::new();
        let key = BackgroundTaskKey::new(BackgroundTaskKind::PhotoMapping, "library-a");
        let queued = manager
            .queue_background("mapping", "photo_mapping", &key)
            .unwrap();
        let task_id = queued.task_id.as_deref().unwrap();
        manager.mark_running(task_id).unwrap();

        let failed = manager
            .finish(task_id, Err("mapping failed".into()))
            .unwrap();

        assert_eq!(failed.state, BackgroundTaskState::Failed);
        assert_eq!(failed.error.as_deref(), Some("mapping failed"));
        assert!(failed.finished_at.is_some());
    }

    #[test]
    fn owner_cancellation_finishes_the_exact_visible_task_as_failed() {
        let registry = ActiveTaskRegistry::default();
        let active = registry.start("formatted-tab".into()).unwrap();
        let cancellation = active.cancellation();
        let manager = OperationManager::new();
        let key = BackgroundTaskKey::new(BackgroundTaskKind::PhotoScan, "formatted-tab");
        let queued = manager
            .queue_background("taxonomy", "preview_taxonomy_rows", &key)
            .unwrap();
        let task_id = queued.task_id.clone().unwrap();
        manager.mark_running(&task_id).unwrap();

        assert_eq!(registry.cancel_owner("formatted-tab").unwrap(), 1);
        let result = cancellation
            .check()
            .map(|_| Value::Null)
            .map_err(|error| error.to_string());
        drop(active);
        let failed = manager.finish(&task_id, result).unwrap();

        assert_eq!(failed.task_id.as_deref(), Some(task_id.as_str()));
        assert_eq!(failed.state, BackgroundTaskState::Failed);
        assert!(
            failed
                .error
                .as_deref()
                .is_some_and(|error| error.contains("cancelled"))
        );
        assert_eq!(manager.operation(&task_id), Some(failed));
    }

    #[test]
    fn completed_task_key_can_be_registered_again_for_new_work() {
        let key = BackgroundTaskKey::new(BackgroundTaskKind::PhotoMapping, "library-a");
        let mut queue = BackgroundQueue::default();
        queue.push(scheduled_work("task-1", key.clone())).unwrap();
        queue.queued.pop_front();
        assert!(queue.complete(&key).is_none());
        queue.push(scheduled_work("task-2", key)).unwrap();

        assert_eq!(queue.queued.len(), 1);
        assert_eq!(
            queue.task_ids.values().next().map(String::as_str),
            Some("task-2")
        );
    }

    #[test]
    fn a_sql_import_operation_blocks_another_sql_import_operation() {
        let mut states = OperationsStatus::new();
        states.insert("task-1".into(), running_state("sql_import"));

        assert_eq!(
            blocked_by(&states, "sql_import").as_deref(),
            Some("sql_import")
        );
    }

    #[test]
    fn sql_and_direct_import_operations_block_each_other() {
        let mut states = OperationsStatus::new();
        states.insert("task-1".into(), running_state("sql_import"));
        assert_eq!(
            blocked_by(&states, "direct_import").as_deref(),
            Some("sql_import")
        );

        states.get_mut("task-1").unwrap().state = BackgroundTaskState::Completed;
        states.insert("task-2".into(), running_state("direct_import"));
        assert_eq!(
            blocked_by(&states, "sql_import").as_deref(),
            Some("direct_import")
        );
    }

    #[test]
    fn queued_background_tasks_do_not_block_the_fifo_head() {
        let mut states = OperationsStatus::new();
        let mut scan = running_state("photos");
        scan.task_id = Some("scan".into());
        scan.state = BackgroundTaskState::Queued;
        let mut metadata = running_state("photos");
        metadata.task_id = Some("metadata".into());
        metadata.state = BackgroundTaskState::Queued;
        let mut mapping = running_state("mapping");
        mapping.task_id = Some("mapping".into());
        mapping.state = BackgroundTaskState::Queued;
        states.insert("scan".into(), scan);
        states.insert("metadata".into(), metadata);
        states.insert("mapping".into(), mapping);

        assert_eq!(
            blocked_by_running_excluding(&states, "photos", Some("scan")),
            None
        );
        assert_eq!(
            blocked_by_running_excluding(&states, "mapping", Some("mapping")),
            None
        );
    }

    #[test]
    fn running_foreground_conflicts_block_background_work() {
        let mut states = OperationsStatus::new();
        states.insert("photos".into(), running_state("photos"));
        assert_eq!(
            blocked_by_running_excluding(&states, "photos", Some("scan")).as_deref(),
            Some("photos")
        );
        states.clear();
        states.insert("mapping".into(), running_state("mapping"));
        assert_eq!(
            blocked_by_running_excluding(&states, "photos", Some("scan")).as_deref(),
            Some("mapping")
        );
        states.get_mut("mapping").unwrap().state = BackgroundTaskState::Completed;
        assert_eq!(
            blocked_by_running_excluding(&states, "photos", Some("scan")),
            None
        );
    }

    #[test]
    fn queued_and_running_states_remain_globally_active() {
        for state in [BackgroundTaskState::Queued, BackgroundTaskState::Running] {
            let mut operation = running_state("photos");
            operation.state = state;
            assert!(operation.is_active());
        }
        for state in [BackgroundTaskState::Completed, BackgroundTaskState::Failed] {
            let mut operation = running_state("photos");
            operation.state = state;
            assert!(!operation.is_active());
        }
    }

    #[test]
    fn photo_pipeline_queue_drains_in_fifo_order() {
        let manager = OperationManager::new();
        let scheduler = BackgroundTaskScheduler::new(manager.clone());
        let executed = Arc::new(Mutex::new(Vec::new()));
        for (kind, module, label) in [
            (BackgroundTaskKind::PhotoScan, "photos", "scan"),
            (BackgroundTaskKind::MetadataIndex, "photos", "metadata"),
            (BackgroundTaskKind::PhotoMapping, "mapping", "mapping"),
        ] {
            let key = BackgroundTaskKey::new(kind, "library-a");
            let state = manager.queue_background(module, label, &key).unwrap();
            let task_id = state.task_id.unwrap();
            let executed = executed.clone();
            scheduler
                .queue
                .lock()
                .unwrap()
                .push(scheduled_work_with_module(
                    &task_id,
                    key,
                    module,
                    Box::new(move |_| {
                        executed.lock().unwrap().push(label);
                        Ok(Value::Null)
                    }),
                ))
                .unwrap();
        }

        loop {
            let work = scheduler.queue.lock().unwrap().queued.pop_front();
            let Some(work) = work else { break };
            assert_eq!(
                manager.blocked_by_running_other(work.pending.module, &work.task_id),
                None
            );
            manager.mark_running(&work.task_id).unwrap();
            let mut progress = |_| {};
            let result = (work.pending.callback)(&mut progress);
            manager.finish(&work.task_id, result).unwrap();
            scheduler.queue.lock().unwrap().complete(&work.pending.key);
        }

        assert_eq!(*executed.lock().unwrap(), ["scan", "metadata", "mapping"]);
        assert!(
            manager
                .status()
                .values()
                .all(|state| { state.state == BackgroundTaskState::Completed })
        );
    }

    #[test]
    fn background_queue_continues_after_a_task_failure() {
        let manager = OperationManager::new();
        let scheduler = BackgroundTaskScheduler::new(manager.clone());
        let continued = Arc::new(Mutex::new(false));
        for (kind, operation, callback) in [
            (
                BackgroundTaskKind::PhotoScan,
                "scan",
                Box::new(|_: &mut (dyn FnMut(OperationProgress) + Send)| Err("scan failed".into()))
                    as BackgroundCallback,
            ),
            (BackgroundTaskKind::MetadataIndex, "metadata", {
                let continued = continued.clone();
                Box::new(move |_| {
                    *continued.lock().unwrap() = true;
                    Ok(Value::Null)
                })
            }),
        ] {
            let key = BackgroundTaskKey::new(kind, "library-a");
            let state = manager.queue_background("photos", operation, &key).unwrap();
            scheduler
                .queue
                .lock()
                .unwrap()
                .push(scheduled_work_with_module(
                    state.task_id.as_deref().unwrap(),
                    key,
                    "photos",
                    callback,
                ))
                .unwrap();
        }

        loop {
            let work = scheduler.queue.lock().unwrap().queued.pop_front();
            let Some(work) = work else { break };
            manager.mark_running(&work.task_id).unwrap();
            let mut progress = |_| {};
            let result = (work.pending.callback)(&mut progress);
            manager.finish(&work.task_id, result).unwrap();
            scheduler.queue.lock().unwrap().complete(&work.pending.key);
        }

        assert!(*continued.lock().unwrap());
        let states = manager.status();
        assert_eq!(
            states
                .values()
                .filter(|state| state.state == BackgroundTaskState::Failed)
                .count(),
            1
        );
        assert_eq!(
            states
                .values()
                .filter(|state| state.state == BackgroundTaskState::Completed)
                .count(),
            1
        );
    }

    fn running_state(module: &str) -> OperationState {
        OperationState {
            module: module.into(),
            task_id: Some(format!("{module}-task")),
            task_kind: None,
            task_scope: None,
            state: BackgroundTaskState::Running,
            operation: Some("test".into()),
            started_at: Some(now()),
            finished_at: None,
            progress: None,
            result: None,
            error: None,
        }
    }

    fn scheduled_work(task_id: &str, key: BackgroundTaskKey) -> ScheduledBackgroundWork {
        scheduled_work_with_module(task_id, key, "photos", Box::new(|_| Ok(Value::Null)))
    }

    fn scheduled_work_with_module(
        task_id: &str,
        key: BackgroundTaskKey,
        module: &'static str,
        callback: BackgroundCallback,
    ) -> ScheduledBackgroundWork {
        ScheduledBackgroundWork {
            task_id: task_id.into(),
            pending: PendingBackgroundWork {
                key,
                module,
                operation: "test",
                callback,
            },
        }
    }

    #[test]
    fn startup_tolerates_a_missing_active_photo_library() {
        let data_dir =
            std::env::temp_dir().join(format!("vividarium-state-{}", Uuid::new_v4().simple()));
        let root = data_dir.join("photos");
        fs::create_dir_all(&root).unwrap();
        let library_path = data_dir.join("library.db");
        {
            let database = Database::open(data_dir.join("metadata.db")).unwrap();
            database
                .register_photo_library(&root, &library_path, Some("Library"))
                .unwrap();
        }
        fs::remove_file(&library_path).unwrap();

        let state = AppState::new(data_dir.clone()).unwrap();

        assert!(state.database.active_photo_library().unwrap().is_some());
        assert!(!library_path.exists());
        fs::remove_dir_all(data_dir).unwrap();
    }
}
