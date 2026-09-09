use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rusqlite::Connection;

use crate::{CoreError, CoreResult};

#[derive(Clone, Default)]
pub struct CancellationToken {
    cancelled: Arc<AtomicBool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }

    pub fn check(&self) -> CoreResult<()> {
        if self.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(crate) fn install_sqlite_progress_handler(&self, connection: &Connection) {
        let cancellation = self.clone();
        connection.progress_handler(1_000, Some(move || cancellation.is_cancelled()));
    }

    pub(crate) fn normalize<T>(&self, result: CoreResult<T>) -> CoreResult<T> {
        if self.is_cancelled() {
            Err(CoreError::Cancelled)
        } else {
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use rusqlite::Connection;

    use super::*;

    #[test]
    fn cancelled_token_interrupts_sqlite_work() {
        let connection = Connection::open_in_memory().unwrap();
        let cancellation = CancellationToken::new();
        cancellation.install_sqlite_progress_handler(&connection);
        cancellation.cancel();

        let result = connection
            .query_row(
                r#"
                WITH RECURSIVE values_to_sum(value) AS (
                    SELECT 1
                    UNION ALL
                    SELECT value + 1 FROM values_to_sum WHERE value < 100000
                )
                SELECT SUM(value) FROM values_to_sum
                "#,
                [],
                |row| row.get::<_, i64>(0),
            )
            .map_err(CoreError::from);

        assert!(matches!(
            cancellation.normalize(result),
            Err(CoreError::Cancelled)
        ));
    }
}
