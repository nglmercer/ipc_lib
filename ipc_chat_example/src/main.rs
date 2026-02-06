//! IPC Chat Example
//! 
//! This example demonstrates a real-time chat application using IPC communication
//! between multiple instances of the application. The primary instance acts as
//! a server that broadcasts messages to all connected secondary instances.
//!
//! Usage:
//!   - Start the first instance (primary): `cargo run --manifest-path ipc_chat_example/Cargo.toml`
//!   - Start additional instances (clients): `cargo run --manifest-path ipc_chat_example/Cargo.toml -- --join`
//!   - Send messages from any instance to broadcast to all others

use single_instance_app::{
    communication::{ProtocolType, CommunicationMessage, CommunicationFactory},
    SingleInstanceApp,
};
use std::env;
use std::process;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::time::{Duration, interval};
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
    WhoIsHere,
    HereIam,
}

/// Chat application state
struct ChatState {
    messages: Arc<Mutex<Vec<ChatMessage>>>,
    clients: Arc<Mutex<Vec<String>>>,
    primary_id: String,
}

impl ChatState {
    fn new(primary_id: String) -> Self {
        Self {
            messages: Arc::new(Mutex::new(Vec::new())),
            clients: Arc::new(Mutex::new(Vec::new())),
            primary_id,
        }
    }

    async fn broadcast(&self, message: ChatMessage) {
        println!("[BROADCAST] {}: {}", message.sender_id, message.content);
        self.messages.lock().await.push(message);
    }

    async fn add_client(&self, client_id: String) {
        self.clients.lock().await.push(client_id.clone());
        println!("[NEW CLIENT] {} joined the chat", client_id);
    }

    async fn get_client_count(&self) -> usize {
        self.clients.lock().await.len()
    }
}

/// Start the chat server (primary instance)
async fn start_server(identifier: &str, username: &str) -> Result<ChatState, Box<dyn std::error::Error>> {
    let state = ChatState::new(process::id().to_string());
    
    let mut app = SingleInstanceApp::new(identifier)
        .with_protocol(ProtocolType::UnixSocket)
        .with_timeout(10000)
        .with_fallback_protocols(vec![
            ProtocolType::FileBased,
            ProtocolType::SharedMemory,
        ]);

    println!("🎯 Starting chat server as primary instance...");
    
    match app.enforce_single_instance().await {
        Ok(true) => {
            println!("✅ Server started successfully!");
            println!("🆔 Server PID: {}", process::id());
            println!("👤 Username: {}", username);
            println!("📍 Endpoint: {}", app.endpoint().unwrap_or_default());
            println!();
            println!("═══ Chat Commands ═══");
            println!("  /who      - List connected clients");
            println!("  /msg <text> - Send a message to all");
            println!("  /history  - Show message history");
            println!("  /quit     - Exit the chat");
            println!("═══════════════════════");
            println!();
            
            // Send system message
            let system_msg = ChatMessage {
                sender_id: "SYSTEM".to_string(),
                content: format!("{} has started the chat server", username),
                timestamp: current_timestamp(),
                message_type: MessageType::System,
            };
            state.broadcast(system_msg).await;
            
            Ok(state)
        }
        Ok(false) => {
            Err("Failed to start server - another instance is running".into())
        }
        Err(e) => {
            Err(format!("Failed to start server: {}", e).into())
        }
    }
}

/// Connect to an existing chat server
async fn connect_to_server(identifier: &str, username: &str) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Connecting to chat server...");
    
    // Try different protocols
    let protocols = vec![
        ProtocolType::UnixSocket,
        ProtocolType::FileBased,
        ProtocolType::SharedMemory,
    ];

    for protocol in protocols {
        println!("  Trying {}...", format!("{:?}", protocol));
        
        let mut app = SingleInstanceApp::new(identifier)
            .with_protocol(protocol)
            .with_timeout(5000);

        match app.enforce_single_instance().await {
            Ok(true) => {
                // We became primary (server was not running)
                println!("✅ Connected! You are now the server.");
                return start_server(identifier, username).await.map(|_| ());
            }
            Ok(false) => {
                // Connected to existing server
                println!("✅ Successfully connected to server!");
                println!("👤 Username: {}", username);
                println!();
                println!("═══ Chat Commands ═══");
                println!("  /who      - List connected clients");
                println!("  /msg <text> - Send a message");
                println!("  /quit     - Exit chat");
                println!("═══════════════════════");
                return Ok(());
            }
            Err(e) => {
                println!("  ❌ {} failed: {}", format!("{:?}", protocol), e);
                continue;
            }
        }
    }

    Err("Could not connect to any server".into())
}

/// Client mode - send and receive messages
async fn run_client_chat(identifier: &str, username: &str) {
    println!("💬 Chat Client - Connected as {}", username);
    println!("Type /quit to exit");
    println!();

    // Simulate receiving messages periodically
    let mut interval = interval(Duration::from_secs(3));
    tokio::spawn(async move {
        loop {
            interval.tick().await;
            // In a real implementation, this would receive messages from the server
        }
    });

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
            match input.split_whitespace().next().unwrap() {
                "/who" => {
                    println!("📋 Connected clients:");
                    println!("  - You ({})", username);
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
            // Send chat message
            println!("📤 You: {}", input);
        }
    }
}

/// Simulate server handling messages
async fn run_server_chat(state: &ChatState, username: &str) {
    println!();
    
    // Simulate receiving messages periodically
    let state = state.clone();
    tokio::spawn(async move {
        let mut interval = interval(Duration::from_secs(5));
        loop {
            interval.tick().await;
            let count = state.get_client_count().await;
            if count > 0 {
                let msg = ChatMessage {
                    sender_id: "SERVER".to_string(),
                    content: format!("{} active connection(s)", count),
                    timestamp: current_timestamp(),
                    message_type: MessageType::System,
                };
                state.broadcast(msg).await;
            }
        }
    });

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
                "/who" => {
                    let count = state.get_client_count().await;
                    println!("📋 {} active connection(s)", count);
                }
                "/msg" => {
                    if parts.len() < 2 {
                        println!("Usage: /msg <message>");
                        continue;
                    }
                    let content = parts[1].to_string();
                    let msg = ChatMessage {
                        sender_id: username.to_string(),
                        content,
                        timestamp: current_timestamp(),
                        message_type: MessageType::Chat,
                    };
                    state.broadcast(msg).await;
                }
                "/history" => {
                    let messages = state.messages.lock().await;
                    println!("📜 Message History ({} messages):", messages.len());
                    for msg in messages.iter().take(10) {
                        println!("  [{}] {}: {}", 
                            msg.timestamp, 
                            msg.sender_id, 
                            msg.content
                        );
                    }
                    if messages.len() > 10 {
                        println!("  ... and {} more", messages.len() - 10);
                    }
                }
                "/quit" | "/exit" => {
                    let msg = ChatMessage {
                        sender_id: "SYSTEM".to_string(),
                        content: format!("{} has left the chat", username),
                        timestamp: current_timestamp(),
                        message_type: MessageType::System,
                    };
                    state.broadcast(msg).await;
                    println!("👋 Goodbye!");
                    break;
                }
                cmd => {
                    println!("Unknown command: {}", cmd);
                }
            }
        } else {
            // Send chat message
            let msg = ChatMessage {
                sender_id: username.to_string(),
                content: input,
                timestamp: current_timestamp(),
                message_type: MessageType::Chat,
            };
            state.broadcast(msg).await;
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

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let identifier = "ipc_chat_example";
    let username = args.get(1)
        .map(|s| s.as_str())
        .unwrap_or_else(|| {
            // Generate random username if not provided
            static ADJECTIVES: &[&str] = &["Happy", "Bright", "Cool", "Swift", "Clever", "Wise", "Bold", "Calm"];
            static NOUNS: &[&str] = &["User", "Chat", "Message", "Talk", "Chat", "Post", "Note", "Line"];
            let adj = ADJECTIVES[process::id() as usize % ADJECTIVES.len()];
            let noun = NOUNS[process::id() as usize % NOUNS.len()];
            let num = process::id() % 1000;
            Box::leak(format!("{}{}{}", adj, noun, num).into_boxed_str()) as &str
        });

    // Check if joining existing server or starting new one
    if args.len() > 1 && args[1] == "--join" {
        // Try to connect to existing server
        match connect_to_server(identifier, username).await {
            Ok(()) => {
                run_client_chat(identifier, username).await;
            }
            Err(e) => {
                eprintln!("❌ Error: {}", e);
                eprintln!("Starting new server instead...");
                match start_server(identifier, username).await {
                    Ok(state) => {
                        run_server_chat(&state, username).await;
                    }
                    Err(e) => {
                        eprintln!("❌ Failed to start server: {}", e);
                        process::exit(1);
                    }
                }
            }
        }
    } {
        // Start as server (primary instance)
        match start_server(identifier, username).await {
            Ok(state) => {
                run_server_chat(&state, username).await;
            }
            Err(e) => {
                eprintln!("❌ Failed to start server: {}", e);
                eprintln!("Trying to connect as client...");
                match connect_to_server(identifier, username).await {
                    Ok(()) => {
                        run_client_chat(identifier, username).await;
                    }
                    Err(e) => {
                        eprintln!("❌ Could not connect: {}", e);
                        process::exit(1);
                    }
                }
            }
        }
    }

    println!("\n👋 Chat session ended");
}
