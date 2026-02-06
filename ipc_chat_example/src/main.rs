//! IPC Chat Example
//! 
//! This example demonstrates a real-time chat application using IPC communication
//! between multiple instances of the application.
//!
//! Usage:
//!   - Start the first instance (primary): `cargo run --manifest-path ipc_chat_example/Cargo.toml`
//!   - Start additional instances (clients): `cargo run --manifest-path ipc_chat_example/Cargo.toml -- --join`
//!   - Send messages that get broadcast to all connected instances

use single_instance_app::{
    communication::ProtocolType,
    SingleInstanceApp,
    IpcClient,
};
use std::env;
use std::process;
use serde::{Deserialize, Serialize};

/// Chat message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    sender_id: String,
    content: String,
    timestamp: u64,
}

/// Client mode - send messages
async fn run_client_chat(username: &str, mut client: Option<IpcClient>) {
    let is_primary = client.is_none();
    println!("💬 Chat session as {} ({})", username, if is_primary { "Host" } else { "Client" });
    println!();
    println!("═══ Chat Commands ═══");
    println!("  <message>   - Send a message to the other instance");
    println!("  /quit       - Exit chat");
    println!("═══════════════════════");
    println!();
    
    // Read user input
    loop {
        print!("> ");
        let _ = std::io::Write::flush(&mut std::io::stdout());
        
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            break;
        }
        
        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        if input == "/quit" || input == "/exit" {
            println!("👋 Goodbye!");
            break;
        }

        if let Some(ref mut c) = client {
            // Secondary instance: send to primary
            let msg = single_instance_app::communication::CommunicationMessage {
                message_type: "chat".to_string(),
                payload: serde_json::json!(input),
                timestamp: current_timestamp(),
                source_id: username.to_string(),
                metadata: serde_json::json!(null),
            };
            
            match c.send_message(msg).await {
                Ok(_) => println!("📤 Sent"),
                Err(e) => println!("❌ Failed to send: {}", e),
            }
        } else {
            // Primary instance: just print locally
            println!("🏠 You (Server): {}", input);
        }
    }
}

fn current_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

#[tokio::main]
async fn main() {
    println!("IPC Chat Application");
    println!();

    let args: Vec<String> = env::args().collect();
    let identifier = "ipc_chat_example";
    
    // Generate username or use provided one
    let username = args.get(1)
        .filter(|s| !s.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            static ADJECTIVES: &[&str] = &["Happy", "Bright", "Cool", "Swift", "Clever"];
            static NOUNS: &[&str] = &["User", "Chat", "Message", "Talk", "Post"];
            let idx = process::id() as usize;
            let adj = ADJECTIVES[idx % ADJECTIVES.len()];
            let noun = NOUNS[idx % NOUNS.len()];
            format!("{}{}{}", adj, noun, process::id() % 1000)
        });

    let mut app = SingleInstanceApp::new(identifier)
        .with_protocol(ProtocolType::UnixSocket)
        .with_timeout(2000);

    println!("🔍 Searching for chat session...");

    match app.enforce_single_instance().await {
        Ok(true) => {
            println!("✅ No existing session found. You are now the host!");
            println!("📍 Endpoint: {}", app.endpoint().unwrap_or_default());
            println!("💡 To see debug logs, run with: IPC_DEBUG=1 cargo run...");
            println!();
            run_client_chat(&username, None).await;
        }
        Ok(false) => {
            println!("✅ Found existing session. Joining...");
            let client = IpcClient::new(identifier)
                .expect("Failed to create client")
                .with_protocol(app.config().protocol);
            run_client_chat(&username, Some(client)).await;
        }
        Err(e) => {
            println!("❌ Failed to initialize: {}", e);
            println!("Try removing /tmp/{}.sock or /tmp/{}.pid if they are stale.", identifier, identifier);
        }
    }

    println!("\n👋 Chat session ended");
}
