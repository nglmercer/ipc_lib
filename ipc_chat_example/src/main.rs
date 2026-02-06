//! IPC Chat Example
//! 
//! This example demonstrates a real-time chat application using IPC communication
//! between multiple instances of the application.

use single_instance_app::{
    communication::ProtocolType,
    communication::CommunicationMessage,
    SingleInstanceApp,
    IpcClient,
};
use std::env;
use std::process;

/// Client mode - send and receive messages
async fn run_client_chat(username: &str, client: Option<IpcClient>, mut host_app: Option<SingleInstanceApp>) {
    let is_host = host_app.is_some();
    println!("💬 Chat session as {} ({})", username, if is_host { "Host" } else { "Client" });
    println!("═══ Chat Commands ═══");
    println!("  <message>   - Send a message to the other instance");
    println!("  /quit       - Exit chat");
    println!("═══════════════════════");
    println!();
    
    if is_host {
        let host = host_app.take().unwrap();
        // Host loop
        loop {
            print!("> ");
            let _ = std::io::Write::flush(&mut std::io::stdout());
            
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_err() {
                break;
            }
            
            let input = input.trim().to_string();
            if input.is_empty() { continue; }
            if input == "/quit" || input == "/exit" { break; }

            let msg = CommunicationMessage {
                message_type: "chat".to_string(),
                payload: serde_json::json!(input),
                timestamp: current_timestamp(),
                source_id: username.to_string(),
                metadata: serde_json::json!(null),
            };

            let _ = host.broadcast(msg).await;
            println!("🏠 You (Host): {}", input);
        }
    } else {
        // Client persistent session
        let client_username = username.to_string();
        let mut session = client.unwrap().connect_persistent().await.expect("Failed to connect");
        

        // We'll use a trick: one task for reading from stdin, one for receiving from socket.
        let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(10);
        
        tokio::spawn(async move {
            loop {
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_ok() {
                    let _ = tx.send(input.trim().to_string()).await;
                }
            }
        });

        loop {
            tokio::select! {
                input = rx.recv() => {
                    if let Some(content) = input {
                        if content == "/quit" || content == "/exit" { break; }
                        let msg = CommunicationMessage {
                            message_type: "chat".to_string(),
                            payload: serde_json::json!(content),
                            timestamp: current_timestamp(),
                            source_id: client_username.clone(),
                            metadata: serde_json::json!(null),
                        };
                        let _ = session.send(msg).await;
                        println!("📤 Sent: {}", content);
                    }
                }
                msg_result = session.receive() => {
                    if let Ok(msg) = msg_result {
                        if msg.message_type == "chat" && msg.source_id != client_username {
                            if let Some(content) = msg.payload.as_str() {
                                println!("\r[CHAT] {}: {}", msg.source_id, content);
                                print!("> ");
                                let _ = std::io::Write::flush(&mut std::io::stdout());
                            }
                        }
                    } else {
                        println!("❌ Connection lost");
                        break;
                    }
                }
            }
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
    
    let username = args.get(1)
        .filter(|s| !s.starts_with("--"))
        .cloned()
        .unwrap_or_else(|| {
            let idx = process::id() as usize;
            format!("User{}", idx % 1000)
        });

    let mut app = SingleInstanceApp::new(identifier)
        .with_protocol(ProtocolType::UnixSocket)
        .with_timeout(60000); // 1 minute timeout for chat

    println!("🔍 Searching for chat session...");

    match app.enforce_single_instance().await {
        Ok(true) => {
            println!("✅ No existing session found. You are now the host!");
            run_client_chat(&username, None, Some(app)).await;
        }
        Ok(false) => {
            println!("✅ Found existing session. Joining...");
            let client = IpcClient::new(identifier)
                .expect("Failed to create client")
                .with_protocol(app.config().protocol);
            run_client_chat(&username, Some(client), None).await;
        }
        Err(e) => {
            println!("❌ Failed to initialize: {}", e);
        }
    }

    println!("\n👋 Chat session ended");
}
