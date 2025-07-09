use realtime::core::state::AppState;
use realtime::core::models::AiAgentWithFeatures;
use realtime::core::queries::{Query, GetAiAgentByNameWithFeatures};
use realtime::agentic::AgentExecutor;
use realtime::core::dto::json::StreamEvent;
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize environment
    dotenvy::dotenv().ok();
    
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter("info,realtime=debug")
        .init();
    
    // Create app state
    let app_state = AppState::new_from_env().await?;
    
    // Get a test agent (you'll need to adjust the agent name)
    let agent_name = "test_agent"; // Replace with actual agent name
    let agent = GetAiAgentByNameWithFeatures::new(agent_name.to_string(), 20220525523509059)
        .execute(&app_state)
        .await?;
    
    if agent.is_none() {
        println!("Agent '{}' not found. Please create an agent first.", agent_name);
        return Ok(());
    }
    
    let agent = agent.unwrap();
    let deployment_id = 20220525523509059; // Replace with actual deployment ID
    let context_id = app_state.sf.next_id()? as i64;
    
    // Create channel for receiving stream events
    let (sender, mut receiver) = mpsc::channel::<StreamEvent>(100);
    
    // Create agent executor
    let mut agent_executor = AgentExecutor::new(
        agent,
        deployment_id,
        context_id,
        app_state.clone(),
        sender,
    ).await?;
    
    // Spawn task to print stream events
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            match event {
                StreamEvent::Token(token) => {
                    println!("Token: {}", token);
                }
                StreamEvent::PlatformEvent(label, data) => {
                    println!("Platform Event [{}]: {:?}", label, data);
                }
                StreamEvent::PlatformFunction(name, result) => {
                    println!("Platform Function [{}]: {:?}", name, result);
                }
            }
        }
    });
    
    // Test message
    let test_message = "Hello, can you help me understand what tools are available?";
    
    println!("\n=== Starting Agent Flow Test ===");
    println!("User Message: {}", test_message);
    println!("\n--- Executing Agent ---\n");
    
    // Execute the agent
    match agent_executor.execute_with_streaming(test_message).await {
        Ok(_) => {
            println!("\n=== Agent Flow Completed Successfully ===");
        }
        Err(e) => {
            println!("\n=== Agent Flow Failed ===");
            println!("Error: {:?}", e);
        }
    }
    
    // Give some time for async tasks to complete
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    Ok(())
}