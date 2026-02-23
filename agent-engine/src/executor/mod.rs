// Executor module - Split into focused submodules for maintainability
//
// This module provides the core agent execution logic:
// - AgentExecutor: Main executor struct that processes user requests
// - AgentExecutorBuilder: Builder pattern for constructing executors
// - ResumeContext: Context for resuming interrupted executions

mod compression;
mod conversation;
mod core;
mod decision;
mod memory;
mod tool_definitions;
mod tool_params;

// Re-export public types
pub use core::{AgentExecutor, AgentExecutorBuilder, ResumeContext};
