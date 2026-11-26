use std::path::Path;
use std::sync::Arc;

use rusqlite::{Connection, TransactionBehavior, params};
use tokio::sync::Mutex;

use crate::{DocId, DocStore, StoreError, merge};

/// Documents in a SQLite database: every update as a row, and the rows of a
/// document made one snapshot whenever it is loaded. Clones share the
/// database.
#[derive(Debug, Clone)]
pub struct SqliteStore(Arc<Mutex<Connection>>);

impl SqliteStore {
    /// Opens the database at `path`, and makes it when there is none.
    /// `":memory:"` is one that goes with the store.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let db = Connection::open(path)?;
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS lotype_updates (
                id INTEGER PRIMARY KEY,
                doc TEXT NOT NULL,
                bytes BLOB NOT NULL
            );
            CREATE INDEX IF NOT EXISTS lotype_updates_doc ON lotype_updates (doc);",
        )?;
        Ok(Self(Arc::new(Mutex::new(db))))
    }

    /// SQLite blocks, so it runs where blocking is allowed.
    async fn run<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut Connection) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, StoreError> {
        // Wait on the executor. The worker owns the guard until the operation
        // ends, including when its caller cancels the wait for the result.
        let mut db = Arc::clone(&self.0).lock_owned().await;
        tokio::task::spawn_blocking(move || work(&mut db)).await?
    }
}

impl DocStore for SqliteStore {
    async fn load(&self, doc: &DocId) -> Result<Vec<Vec<u8>>, StoreError> {
        let doc = doc.as_str().to_owned();
        self.run(move |db| {
            // To write from the start: another connection to the file that
            // wrote after a read here would fail the rows' replacement.
            let db = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let rows: Vec<Vec<u8>> = db
                .prepare("SELECT bytes FROM lotype_updates WHERE doc = ?1 ORDER BY id")?
                .query_map([&doc], |row| row.get(0))?
                .collect::<Result<_, _>>()?;
            if rows.len() < 2 {
                return Ok(rows);
            }
            let merged = merge(&rows)?;
            db.execute("DELETE FROM lotype_updates WHERE doc = ?1", [&doc])?;
            for bytes in &merged {
                db.execute(
                    "INSERT INTO lotype_updates (doc, bytes) VALUES (?1, ?2)",
                    params![doc, bytes],
                )?;
            }
            db.commit()?;
            Ok(merged)
        })
        .await
    }

    async fn append_update(&self, doc: &DocId, update: &[u8]) -> Result<(), StoreError> {
        let (doc, update) = (doc.as_str().to_owned(), update.to_vec());
        self.run(move |db| {
            db.execute(
                "INSERT INTO lotype_updates (doc, bytes) VALUES (?1, ?2)",
                params![doc, update],
            )?;
            Ok(())
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::future::poll_fn;
    use std::sync::mpsc;
    use std::task::Poll;
    use std::time::Duration;

    use super::SqliteStore;

    #[test]
    fn waiting_for_sqlite_leaves_a_worker_free_after_the_caller_cancels() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .max_blocking_threads(2)
            .enable_time()
            .build()
            .unwrap();
        runtime.block_on(async {
            let store = SqliteStore::open(":memory:").unwrap();
            let (entered, running) = tokio::sync::oneshot::channel();
            let (release, wait) = mpsc::channel();
            let first_store = store.clone();
            let first = tokio::spawn(async move {
                first_store
                    .run(move |_| {
                        entered.send(()).unwrap();
                        wait.recv().unwrap();
                        Ok(())
                    })
                    .await
            });
            running.await.unwrap();
            first.abort();
            assert!(first.await.unwrap_err().is_cancelled());

            let mut queued = std::pin::pin!(store.run(|_| Ok(())));
            let waiting = poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx).is_pending())).await;
            let spare =
                tokio::time::timeout(Duration::from_secs(2), tokio::task::spawn_blocking(|| ()))
                    .await;
            release.send(()).unwrap();

            assert!(waiting);
            assert!(matches!(spare, Ok(Ok(()))));
            queued.await.unwrap();
        });
    }
}
