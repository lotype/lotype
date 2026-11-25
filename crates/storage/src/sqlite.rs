use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::{Connection, TransactionBehavior, params};

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
        let db = Arc::clone(&self.0);
        let work = move || work(&mut db.lock().unwrap_or_else(PoisonError::into_inner));
        tokio::task::spawn_blocking(work).await?
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
