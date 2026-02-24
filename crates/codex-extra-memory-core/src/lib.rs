pub mod agents_sync;
pub mod autocapture;
pub mod commands;
pub mod config;
pub mod llm;
pub mod paths;
pub mod render;
pub mod scope;
pub mod service;
pub mod store;
pub mod types;
pub mod utils;

pub use config::MemoryConfig;
pub use service::MemoryService;
pub use types::{
    AddMemoryInput, AddMemoryResult, AutoCaptureCandidate, AutoCaptureConfig, CompactionMode,
    CompactionResult, MemoryCategory, MemoryRow, MemoryStats, ResolveIdResult, ScopeInfo,
    ScopeTarget, SyncAgentsResult,
};
