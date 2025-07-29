use serde::{Deserialize, Serialize};

use super::conversation::ConversationRecord;
use super::memory::MemoryRecord;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImmediateContext {
    pub memories: Vec<MemoryRecord>,
    pub conversations: Vec<ConversationRecord>,
}
