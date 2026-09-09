use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, ipc::Channel};
use tauri_plugin_updater::{Update, UpdaterExt};
use vividarium_core::OperationsStatus;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppUpdateInfo {
    pub current_version: String,
    pub version: String,
    pub notes: Option<String>,
    pub published_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "event", content = "data", rename_all = "snake_case")]
pub enum AppUpdateEvent {
    Started {
        content_length: Option<u64>,
    },
    Progress {
        chunk_length: usize,
        downloaded: u64,
    },
    Finished,
}

#[derive(Default)]
pub struct PendingAppUpdate(Mutex<Option<Update>>);

pub(crate) fn ensure_install_allowed(operations: &OperationsStatus) -> Result<(), String> {
    let running = operations
        .values()
        .filter(|state| state.is_active())
        .map(|state| {
            state.operation.as_deref().map_or_else(
                || state.module.clone(),
                |operation| format!("{}/{operation}", state.module),
            )
        })
        .collect::<Vec<_>>();
    if running.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "app update installation is blocked by running operations: {}",
            running.join(", ")
        ))
    }
}

pub async fn check(
    app: &AppHandle,
    pending: &PendingAppUpdate,
) -> Result<Option<AppUpdateInfo>, String> {
    let update = app
        .updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())?;
    let info = update.as_ref().map(|update| AppUpdateInfo {
        current_version: update.current_version.clone(),
        version: update.version.clone(),
        notes: update.body.clone(),
        published_at: update.date.map(|date| date.to_string()),
    });
    *pending
        .0
        .lock()
        .map_err(|_| "pending app update lock is poisoned".to_string())? = update;
    Ok(info)
}

pub async fn install(
    app: &AppHandle,
    pending: &PendingAppUpdate,
    on_event: Channel<AppUpdateEvent>,
) -> Result<(), String> {
    let update = pending
        .0
        .lock()
        .map_err(|_| "pending app update lock is poisoned".to_string())?
        .take()
        .ok_or_else(|| "there is no pending app update".to_string())?;
    let mut started = false;
    let mut downloaded = 0u64;
    let result = update
        .download_and_install(
            |chunk_length, content_length| {
                if !started {
                    let _ = on_event.send(AppUpdateEvent::Started { content_length });
                    started = true;
                }
                downloaded += chunk_length as u64;
                let _ = on_event.send(AppUpdateEvent::Progress {
                    chunk_length,
                    downloaded,
                });
            },
            || {
                let _ = on_event.send(AppUpdateEvent::Finished);
            },
        )
        .await;
    if let Err(error) = result {
        *pending
            .0
            .lock()
            .map_err(|_| "pending app update lock is poisoned".to_string())? = Some(update);
        return Err(error.to_string());
    }
    app.restart()
}

#[cfg(test)]
mod tests {
    use super::{AppUpdateEvent, AppUpdateInfo, ensure_install_allowed};
    use serde_json::json;
    use vividarium_core::{OperationState, OperationsStatus};

    #[test]
    fn serializes_update_info_for_ipc() {
        let info = AppUpdateInfo {
            current_version: "3.1.0".to_string(),
            version: "3.1.1".to_string(),
            notes: Some("Release notes".to_string()),
            published_at: Some("2026-07-26T00:00:00Z".to_string()),
        };

        assert_eq!(
            serde_json::to_value(info).unwrap(),
            json!({
                "current_version": "3.1.0",
                "version": "3.1.1",
                "notes": "Release notes",
                "published_at": "2026-07-26T00:00:00Z"
            })
        );
    }

    #[test]
    fn serializes_download_events_for_ipc() {
        assert_eq!(
            serde_json::to_value(AppUpdateEvent::Started {
                content_length: Some(1024),
            })
            .unwrap(),
            json!({
                "event": "started",
                "data": {
                    "content_length": 1024
                }
            })
        );
        assert_eq!(
            serde_json::to_value(AppUpdateEvent::Progress {
                chunk_length: 256,
                downloaded: 768,
            })
            .unwrap(),
            json!({
                "event": "progress",
                "data": {
                    "chunk_length": 256,
                    "downloaded": 768
                }
            })
        );
        assert_eq!(
            serde_json::to_value(AppUpdateEvent::Finished).unwrap(),
            json!({
                "event": "finished"
            })
        );
    }

    #[test]
    fn install_guard_rejects_running_operations() {
        let mut operations = OperationsStatus::new();
        operations.insert(
            "mapping".into(),
            OperationState {
                module: "mapping".into(),
                task_id: Some("task".into()),
                task_kind: None,
                task_scope: None,
                state: vividarium_core::BackgroundTaskState::Running,
                operation: Some("match".into()),
                started_at: None,
                finished_at: None,
                progress: None,
                result: None,
                error: None,
            },
        );

        let error = ensure_install_allowed(&operations).unwrap_err();

        assert_eq!(
            error,
            "app update installation is blocked by running operations: mapping/match"
        );
    }

    #[test]
    fn install_guard_accepts_idle_operations() {
        let mut operations = OperationsStatus::new();
        operations.insert(
            "photos".into(),
            OperationState {
                module: "photos".into(),
                task_id: None,
                task_kind: None,
                task_scope: None,
                state: vividarium_core::BackgroundTaskState::Completed,
                operation: None,
                started_at: None,
                finished_at: None,
                progress: None,
                result: None,
                error: None,
            },
        );

        ensure_install_allowed(&operations).unwrap();
    }
}
