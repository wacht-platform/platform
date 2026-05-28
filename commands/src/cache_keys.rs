pub fn ai_settings(deployment_id: i64) -> String {
    format!("agent:cache:ai_settings:{deployment_id}")
}

pub fn provider_profiles(deployment_id: i64) -> String {
    format!("agent:cache:provider_profiles:{deployment_id}")
}

pub fn mcp_actor_bundle(deployment_id: i64, actor_id: i64) -> String {
    format!("agent:cache:mcp_actor:{deployment_id}:{actor_id}")
}
