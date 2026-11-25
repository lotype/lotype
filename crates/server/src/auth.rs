use std::future::Future;

use crate::DocId;

/// What a client may do with a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Get the document and the changes to it.
    Read,
    /// Change it as well.
    Write,
}

/// The token is not one that opens the document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthError;

/// Says what a client may do. Tokens are the application's: it issues them
/// and it knows them here.
pub trait Authorizer: Send + Sync + 'static {
    /// What the holder of `token` may do with `doc`, which need not exist yet.
    fn authorize(
        &self,
        token: &str,
        doc: &DocId,
    ) -> impl Future<Output = Result<Access, AuthError>> + Send;
}
