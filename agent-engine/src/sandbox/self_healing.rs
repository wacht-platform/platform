use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use super::{ExecRequest, ExecResult, SandboxError, SandboxHandle, SandboxResult};

pub type RecreateFuture =
    Pin<Box<dyn Future<Output = SandboxResult<Arc<dyn SandboxHandle>>> + Send>>;
pub type RecreateFn = Arc<dyn Fn() -> RecreateFuture + Send + Sync>;

/// Wraps a `SandboxHandle` so that when an operation fails with
/// `SandboxError::NotFound` (the sandbox was evicted on the node), the wrapper
/// transparently calls the supplied `recreate` closure to obtain a fresh handle
/// and retries the operation exactly once.
///
/// The task workspace (`/task/`) is S3-backed via rclone, so the recreated
/// sandbox sees the same artifacts. State that was fsynced before eviction
/// survives; in-flight buffered writes do not (existing rclone behavior).
pub struct SelfHealingHandle {
    inner: Mutex<Arc<dyn SandboxHandle>>,
    recreate: RecreateFn,
    label: String,
}

impl SelfHealingHandle {
    pub fn new(initial: Arc<dyn SandboxHandle>, recreate: RecreateFn, label: String) -> Self {
        Self {
            inner: Mutex::new(initial),
            recreate,
            label,
        }
    }

    async fn current(&self) -> Arc<dyn SandboxHandle> {
        self.inner.lock().await.clone()
    }

    async fn refresh(&self) -> SandboxResult<Arc<dyn SandboxHandle>> {
        let fresh = (self.recreate)().await?;
        tracing::warn!(
            label = %self.label,
            new_sandbox_id = %fresh.id(),
            "sandbox: self-healing recreated after NotFound",
        );
        *self.inner.lock().await = fresh.clone();
        Ok(fresh)
    }
}

#[async_trait]
impl SandboxHandle for SelfHealingHandle {
    fn id(&self) -> &str {
        // We can't easily return the inner id without blocking on a mutex inside
        // a `&self -> &str` boundary. Return the stable label instead; callers
        // that need the current sandbox id should treat this as advisory.
        &self.label
    }

    async fn exec(&self, request: ExecRequest) -> SandboxResult<ExecResult> {
        let handle = self.current().await;
        match handle.exec(request.clone()).await {
            Err(SandboxError::NotFound(detail)) => {
                tracing::debug!(
                    label = %self.label,
                    detail = %detail,
                    "sandbox: exec hit NotFound, recreating and retrying once",
                );
                let fresh = self.refresh().await?;
                fresh.exec(request).await
            }
            other => other,
        }
    }

    async fn cancel(&self, exec_id: &str) -> SandboxResult<bool> {
        // Cancelling a vanished exec on a vanished sandbox is a no-op; treat
        // NotFound as success without recreating (recreating just to cancel
        // would waste cycles).
        let handle = self.current().await;
        match handle.cancel(exec_id).await {
            Err(SandboxError::NotFound(_)) => Ok(false),
            other => other,
        }
    }

    async fn delete(&self) -> SandboxResult<()> {
        let handle = self.current().await;
        match handle.delete().await {
            // Deleting a sandbox that the node already evicted is a no-op.
            Err(SandboxError::NotFound(_)) => Ok(()),
            other => other,
        }
    }

    async fn touch(&self) -> SandboxResult<()> {
        let handle = self.current().await;
        match handle.touch().await {
            Err(SandboxError::NotFound(detail)) => {
                tracing::debug!(
                    label = %self.label,
                    detail = %detail,
                    "sandbox: touch hit NotFound, recreating and retrying once",
                );
                let fresh = self.refresh().await?;
                fresh.touch().await
            }
            other => other,
        }
    }

    async fn reconcile_skills(
        &self,
        agent_id: &str,
        slugs: Vec<String>,
    ) -> SandboxResult<Vec<String>> {
        let handle = self.current().await;
        match handle.reconcile_skills(agent_id, slugs.clone()).await {
            Err(SandboxError::NotFound(detail)) => {
                tracing::debug!(
                    label = %self.label,
                    detail = %detail,
                    "sandbox: reconcile_skills hit NotFound, recreating and retrying once",
                );
                let fresh = self.refresh().await?;
                fresh.reconcile_skills(agent_id, slugs).await
            }
            other => other,
        }
    }

    async fn read_file(&self, path: &str) -> SandboxResult<Vec<u8>> {
        let handle = self.current().await;
        match handle.read_file(path).await {
            Err(SandboxError::NotFound(detail)) if !detail.starts_with("read ") => {
                // NotFound from the sandbox layer (vs. the file-missing NotFound
                // emitted by the default read_file helper) triggers a recreate.
                tracing::debug!(
                    label = %self.label,
                    detail = %detail,
                    "sandbox: read_file hit NotFound, recreating and retrying once",
                );
                let fresh = self.refresh().await?;
                fresh.read_file(path).await
            }
            other => other,
        }
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> SandboxResult<()> {
        let handle = self.current().await;
        match handle.write_file(path, content).await {
            Err(SandboxError::NotFound(detail)) => {
                tracing::debug!(
                    label = %self.label,
                    detail = %detail,
                    "sandbox: write_file hit NotFound, recreating and retrying once",
                );
                let fresh = self.refresh().await?;
                fresh.write_file(path, content).await
            }
            other => other,
        }
    }
}
