use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use futures::future::BoxFuture;
use sqlx::{PgPool, Postgres, Transaction};

use crate::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadConsistency {
    Eventual,
    Strong,
}

#[derive(Clone)]
pub struct DbRouter {
    writer: PgPool,
    readers: Arc<Vec<PgPool>>,
    next_reader: Arc<AtomicUsize>,
}

impl DbRouter {
    pub fn new(writer: PgPool, readers: Vec<PgPool>) -> Self {
        Self {
            writer,
            readers: Arc::new(readers),
            next_reader: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn writer(&self) -> &PgPool {
        &self.writer
    }

    pub fn reader(&self, consistency: ReadConsistency) -> &PgPool {
        match consistency {
            ReadConsistency::Strong => &self.writer,
            ReadConsistency::Eventual => {
                if self.readers.is_empty() {
                    return &self.writer;
                }

                let idx = self.next_reader.fetch_add(1, Ordering::Relaxed) % self.readers.len();
                &self.readers[idx]
            }
        }
    }

    pub fn has_readers(&self) -> bool {
        !self.readers.is_empty()
    }

    pub async fn with_tx<T>(
        &self,
        f: impl for<'tx> FnOnce(
            &'tx mut Transaction<'_, Postgres>,
        ) -> BoxFuture<'tx, Result<T, AppError>>,
    ) -> Result<T, AppError> {
        let mut tx = self.writer.begin().await?;
        let out = f(&mut tx).await?;
        tx.commit().await?;
        Ok(out)
    }
}
