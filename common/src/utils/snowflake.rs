use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Generate a unique ID using a simplified snowflake algorithm
pub fn generate_id() -> i64 {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    
    let sequence = SEQUENCE.fetch_add(1, Ordering::SeqCst) & 0xFFF; // 12 bits for sequence
    
    // Simplified snowflake: 41 bits timestamp + 12 bits sequence + 10 bits machine/worker (set to 1)
    let id = (timestamp << 22) | (1 << 12) | sequence;
    
    id as i64
}


