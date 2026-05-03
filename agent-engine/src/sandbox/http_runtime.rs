use async_trait::async_trait;

use super::blueprint::HttpBlueprint;
use super::{
    SandboxError, SandboxHandle, SandboxResult, SandboxRuntime, TaskSandboxSpec, ThreadSandboxSpec,
};

pub struct HttpSandboxRuntime {
    _blueprint: HttpBlueprint,
}

impl HttpSandboxRuntime {
    pub fn new(blueprint: HttpBlueprint) -> Self {
        Self {
            _blueprint: blueprint,
        }
    }
}

#[async_trait]
impl SandboxRuntime for HttpSandboxRuntime {
    async fn ensure_thread_sandbox(
        &self,
        _spec: ThreadSandboxSpec,
    ) -> SandboxResult<Box<dyn SandboxHandle>> {
        Err(SandboxError::Config(
            "HttpSandboxRuntime is not implemented yet".into(),
        ))
    }

    async fn ensure_task_sandbox(
        &self,
        _spec: TaskSandboxSpec,
    ) -> SandboxResult<Box<dyn SandboxHandle>> {
        Err(SandboxError::Config(
            "HttpSandboxRuntime is not implemented yet".into(),
        ))
    }
}
