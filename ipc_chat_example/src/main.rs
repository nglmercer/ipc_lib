//! IPC Chat Example
//! 
//! This example demonstrates a real-time chat application using IPC communication
//! between multiple instances of the application.

use single_instance_app::{
    communication::ProtocolType,
    communication::CommunicationMessage,
    current_timestamp,
    IpcClient,
    SingleInstanceApp,
};
use std::env;
use std::process;

async fn run_client_chat(username: &str, client: Option<IpcClient>, host_app: Option<SingleInstanceApp>) {
    let is_host = host_app.is_some();
    println!("💬 Chat session as {} ({})", username, if is_host { "Host" } else { "Client" });
    println!("═══ Chat Commands ═══");
    println!("  <message>   - Send a message to the other instance");
    println!("  /quit       - Exit chat");
    println!("═══════════════════════");
    println!();
    
    let (stdin_tx, mut stdin_rx) = tokio::sync::mpsc::channel::<String>(100);
    
    // Background task for stdin
    tokio::spawn(async move {
        loop {
            let mut input = String::new();
            if std::io::stdin().read_line(&mut input).is_ok() {
                let text = input.trim().to_string();
                if !text.is_empty() {
                    let _ = stdin_tx.send(text).await;
                }
            } else {
                break;
            }
        }
    });

    if is_host {
        let host = host_app.unwrap();
        print!("> ");
        let _ = std::io::Write::flush(&mut std::io::stdout());

        loop {
            tokio::select! {
                input = stdin_rx.recv() => {
                    if let Some(content) = input {
                        if content == "/quit" || content == "/exit" { break; }
                        let msg = CommunicationMessage {
                            message_type: "chat".to_string(),
                            payload: serde_json::json!(content),
                            timestamp: current_timestamp(),
                            source_id: username.to_string(),
                            metadata: serde_json::json!(null),
                        };
                        let _ = host.broadcast(msg).await;
                        println!("🏠 You (Host): {}", content);
                        print!("> ");
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                }
                // Host receives messages via the on_message handler registered in main()
                // so we don't need a receive loop here for the host.
            }
        }
    } else {
        let mut session = client.unwrap().connect_persistent().await.expect("Failed to connect");
        let client_username = username.to_string();
        print!("> ");
        let _ = std::io::Write::flush(&mut std::io::stdout());

        loop {
            tokio::select! {
                input = stdin_rx.recv() => {
                    if let Some(content) = input {
                        if content == "/quit" || content == "/exit" { break; }
                        let msg = CommunicationMessage {
                            message_type: "chat".to_string(),
                            payload: serde_json::json!(content),
                            timestamp: current_timestamp(),
                            source_id: client_username.clone(),
                            metadata: serde_json::json!(null),
                        };
                        
                        // Try to send, and reconnect if it fails
                        if let Err(_) = session.send(msg).await {
                             println!("\r⚠️ Send failed, attempting to reconnect...");
                             if session.reconnect().await.is_ok() {
                                 println!("✅ Reconnected! Resending...");
                                 // Try resending once
                                 let msg_retry = CommunicationMessage {
                                     message_type: "chat".to_string(),
                                     payload: serde_json::json!(content),
                                     timestamp: current_timestamp(),
                                     source_id: client_username.clone(),
                                     metadata: serde_json::json!(null),
                                 };
                                 let _ = session.send(msg_retry).await;
                             } else {
                                 println!("❌ Reconnect failed. Connection lost.");
                                 break;
                             }
                        }
                        
                        println!("📤 Sent: {}", content);
                        print!("> ");
                        let _ = std::io::Write::flush(&mut std::io::stdout());
                    }
                }
                msg_result = session.receive() => {
                    match msg_result {
                        Ok(msg) => {
                            if msg.message_type == "chat" && msg.source_id != client_username {
                                if let Some(content) = msg.payload.as_str() {
                                    println!("\r[CHAT] {}: {}", msg.source_id, content);
                                    print!("> ");
                                    let _ = std::io::Write::flush(&mut std::io::stdout());
                                }
                            }
                        }
                        Err(_) => {
                            println!("\r⚠️ Connection lost. Attempting to reconnect...");
                            // Try to reconnect a few times
                            let mut reconnected = false;
                            for i in 1..=3 {
                                tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                                println!("Attempt {}/3...", i);
                                if session.reconnect().await.is_ok() {
                                    println!("✅ Reconnected!");
                                    reconnected = true;
                                    break;
                                }
                            }
                            
                            if !reconnected {
                                println!("❌ Could not reconnect.");
                                break;
                            }
                            print!("> ");
                            let _ = std::io::Write::flush(&mut std::io::stdout());
                        }
                    }
                }
            }
        }
    }
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
    
    // Check if logging should be enabled
    if args.iter().any(|arg| arg == "--log") {
        single_instance_app::enable_logging();
        println!("📝 Logging enabled");
    }

    let username = args.get(1)
        .filter(|s| !s.starts_with("--") && *s != "join")
        .cloned()
        .unwrap_or_else(|| {
            let idx = process::id() as usize;
            format!("User{}", idx % 1000)
        });

    let username_clone = username.clone();
    let mut app = SingleInstanceApp::new(identifier)
        .with_protocol(ProtocolType::UnixSocket)
        .on_message(move |msg| {
            if msg.message_type == "chat" && msg.source_id != username_clone {
                if let Some(content) = msg.payload.as_str() {
                    println!("\r[CHAT] {}: {}", msg.source_id, content);
                    print!("> ");
                    let _ = std::io::Write::flush(&mut std::io::stdout());
                }
            }
        });

    println!("🔍 Initializing chat session...");

    match app.enforce_single_instance().await {
        Ok(true) => {
            println!("🏠 You are the host!");
            run_client_chat(&username, None, Some(app)).await;
        }
        Ok(false) => {
            println!("💬 Joining existing session...");
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
