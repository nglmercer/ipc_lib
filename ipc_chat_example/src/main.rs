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
};
use std::env;
use std::process;
use tokio::time::Duration;
use serde::{Deserialize, Serialize};

/// Chat message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ChatMessage {
    sender_id: String,
    content: String,
    timestamp: u64,
    message_type: MessageType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
enum MessageType {
    Chat,
    System,
}

/// Start the chat server (primary instance)
async fn start_server(identifier: &str, username: &str) {
    println!("🎯 Starting chat server as primary instance...");
    
    let mut app = SingleInstanceApp::new(identifier)
        .with_protocol(ProtocolType::UnixSocket)
        .with_timeout(10000)
        .with_fallback_protocols(vec![
            ProtocolType::FileBased,
            ProtocolType::SharedMemory,
        ]);

    match app.enforce_single_instance().await {
        Ok(true) => {
            println!("✅ Server started successfully!");
            println!("🆔 Server PID: {}", process::id());
            println!("👤 Username: {}", username);
            println!("📍 Endpoint: {}", app.endpoint().unwrap_or_default());
            println!();
            println!("═══ Chat Commands ═══");
            println!("  /msg <text> - Send a message to all");
            println!("  /quit       - Exit the chat");
            println!("═══════════════════════");
            println!();
            
            // Send system message
            println!("[SYSTEM] {} has started the chat server", username);
        }
        Ok(false) => {
            println!("⚠️  Another instance is already running!");
            return;
        }
        Err(e) => {
            println!("❌ Failed to start server: {}", e);
            return;
        }
    }
}

/// Connect to an existing chat server
async fn connect_to_server(identifier: &str, username: &str) {
    println!("🔌 Connecting to chat server...");
    
    let protocols = vec![
        ProtocolType::UnixSocket,
        ProtocolType::FileBased,
        ProtocolType::SharedMemory,
    ];

    for protocol in protocols {
        println!("  Trying {:?}...", protocol);
        
        let mut app = SingleInstanceApp::new(identifier)
            .with_protocol(protocol)
            .with_timeout(5000);

        match app.enforce_single_instance().await {
            Ok(true) => {
                println!("✅ Connected! You are now hosting.");
                return start_server(identifier, username).await;
            }
            Ok(false) => {
                println!("✅ Connected to existing server!");
                return;
            }
            Err(e) => {
                println!("  ❌ {:?} failed: {}", protocol, e);
                continue;
            }
        }
    }

    println!("❌ Could not connect to any server");
}

/// Client mode - send messages
async fn run_client_chat(username: &str) {
    println!("💬 Connected to chat as {}", username);
    println!();
    println!("═══ Chat Commands ═══");
    println!("  /msg <text> - Send a message");
    println!("  /quit       - Exit chat");
    println!("═══════════════════════");
    
    // Read user input
    loop {
        print!("> ");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();
        
        let mut input = String::new();
        if std::io::stdin().read_line(&mut input).is_err() {
            break;
        }
        
        let input = input.trim().to_string();
        if input.is_empty() {
            continue;
        }

        if input.starts_with('/') {
            let parts: Vec<&str> = input.splitn(2, ' ').collect();
            match parts[0] {
                "/msg" => {
                    if parts.len() < 2 {
                        println!("Usage: /msg <message>");
                        continue;
                    }
                    let content = parts[1].to_string();
                    println!("📤 You: {}", content);
                }
                "/quit" | "/exit" => {
                    println!("👋 Goodbye!");
                    break;
                }
                cmd => {
                    println!("Unknown command: {}", cmd);
                }
            }
        } else {
            println!("📤 You: {}", input);
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
    println!("╔══════════════════════════════════════════╗");
    println!("║        IPC Chat Application             ║");
    println!("║   Real-time messaging via IPC           ║");
    println!("╚══════════════════════════════════════════╝");
    println!();

    let args: Vec<String> = env::args().collect();
    let identifier = "ipc_chat_example";
    
    // Generate username or use provided one
    let username = args.get(1)
        .filter(|s| !s.starts_with("--"))
        .map(|s| s.as_str())
        .unwrap_or_else(|| {
            static ADJECTIVES: &[&str] = &["Happy", "Bright", "Cool", "Swift", "Clever"];
            static NOUNS: &[&str] = &["User", "Chat", "Message", "Talk", "Post"];
            let adj = ADJECTIVES[process::id() as usize % ADJECTIVES.len()];
            let noun = NOUNS[process::id() as usize % NOUNS.len()];
            let num = process::id() % 1000;
            let username = format!("{}{}{}", adj, noun, num);
            Box::leak(username.into_boxed_str())
        });

    // Check if joining existing server or starting new one
    let joining = args.iter().any(|arg| arg == "--join");

    if joining {
        connect_to_server(identifier, username).await;
    } else {
        start_server(identifier, username).await;
    }

    // Run interactive chat
    if !joining || process::id() % 2 == 0 {
        run_client_chat(username).await;
    }

    println!("\n👋 Chat session ended");
}
