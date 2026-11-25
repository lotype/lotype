//! Where lotype documents are kept: the store a server saves to, and one in SQLite.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

#[cfg(feature = "sqlite")]
mod sqlite;

use std::future::Future;
use std::panic::{AssertUnwindSafe, catch_unwind};

use lotype_core::{Document, Schema};

#[cfg(feature = "sqlite")]
pub use sqlite::SqliteStore;

/// What a store could not do.
pub type StoreError = Box<dyn std::error::Error + Send + Sync>;

/// The name an application gives a document.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DocId(String);

impl DocId {
    /// The document named `id`.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Keeps documents for a server, as the bytes peers exchange.
pub trait DocStore: Send + Sync + 'static {
    /// Everything stored of the document, as snapshots and updates to
    /// import in any order, and none when nothing is. A store need not
    /// keep each update apart: [`merge`] makes one snapshot of them.
    ///
    /// A server takes bytes that are no document for a failure, and a load
    /// that fails for a document it cannot open.
    fn load(&self, doc: &DocId) -> impl Future<Output = Result<Vec<Vec<u8>>, StoreError>> + Send;

    /// Keeps an update with what is stored of the document. A server has
    /// checked that it is one.
    fn append_update(
        &self,
        doc: &DocId,
        update: &[u8],
    ) -> impl Future<Output = Result<(), StoreError>> + Send;
}

/// Loro checks what it imports, but bytes can be made that it panics on,
/// and it panics again when a document it panicked in is dropped. That is
/// an error here, and the document is left where it is, never dropped.
fn guarded<T>(
    mut doc: Document,
    work: impl FnOnce(&mut Document) -> lotype_core::Result<T>,
) -> Result<(Document, T), StoreError> {
    match catch_unwind(AssertUnwindSafe(|| work(&mut doc))) {
        Ok(done) => Ok((doc, done?)),
        Err(_) => {
            std::mem::forget(doc);
            Err("Loro panicked on what is stored".into())
        }
    }
}

/// The document that the snapshots and updates in `parts` make together,
/// in any order. Bytes that are none, or that Loro panics on, are an error.
pub fn restore<P: AsRef<[u8]>>(parts: impl IntoIterator<Item = P>) -> Result<Document, StoreError> {
    // Blocks of any type are kept, so the schema does not matter here.
    let doc = Document::new(Schema::new());
    let import = |doc: &mut Document| {
        let mut parts = parts.into_iter();
        parts.try_for_each(|part| doc.import(part.as_ref()))
    };
    guarded(doc, import).map(|(doc, ())| doc)
}

/// The fewest bytes that hold what the snapshots and updates in `parts`
/// do: one snapshot, and after it, if any, the changes among them that wait
/// for others not among them, which are in no snapshot.
pub fn merge<P: AsRef<[u8]>>(
    parts: impl IntoIterator<Item = P>,
) -> Result<Vec<Vec<u8>>, StoreError> {
    let export = |doc: &mut Document| Ok([Some(doc.export_snapshot()?), doc.export_waiting()?]);
    let (_, merged) = guarded(restore(parts)?, export)?;
    Ok(merged.into_iter().flatten().collect())
}
