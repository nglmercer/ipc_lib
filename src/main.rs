use single_instance_app::{
    communication::ProtocolType,
    SingleInstanceApp,
};
use std::env;
use std::process;

/// Basic example demonstrating single instance application with IPC communication
#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    
    // Check if we're being called as a secondary instance
    if args.len() > 1 && args[1] == "--secondary" {
        handle_secondary_instance(&args[2..]).await;
        return;
    }
    
    let identifier = "my_single_instance_app";
    
    // Create and configure the single instance application
    let mut app = SingleInstanceApp::new(identifier)
        .with_protocol(ProtocolType::UnixSocket)  // Try Unix sockets first
        .with_timeout(5000)                        // 5 second timeout
        .without_fallback();                       // Disable fallback for this example
    
    // Enforce single instance
    match app.enforce_single_instance().await {
        Ok(true) => {
            println!("Primary instance started successfully");
            println!("PID: {}", process::id());
            println!("Endpoint: {}", app.endpoint().unwrap_or_default());
            
            // Simulate primary instance work
            primary_instance_work();
        }
        Ok(false) => {
            println!("Secondary instance, exiting");
            process::exit(0);
        }
        Err(e) => {
            eprintln!("Failed to enforce single instance: {}", e);
            
            // Try with file-based protocol as fallback
            println!("Trying file-based protocol...");
            let mut app_fallback = SingleInstanceApp::new(identifier)
                .with_protocol(ProtocolType::FileBased)
                .with_timeout(5000);
            
            match app_fallback.enforce_single_instance().await {
                Ok(true) => {
                    println!("Primary instance started with file-based protocol");
                    primary_instance_work();
                }
                Ok(false) => {
                    println!("Secondary instance (file-based), exiting");
                    process::exit(0);
                }
                Err(e_fallback) => {
                    eprintln!("Failed with file-based protocol too: {}", e_fallback);
                    process::exit(1);
                }
            }
        }
    }
}

fn primary_instance_work() {
    println!("Primary instance is running...");
    println!("Press Ctrl+C to exit");
    
    // Keep the application running
    std::thread::park();
}

async fn handle_secondary_instance(_args: &[String]) {
    let identifier = "my_single_instance_app";
    
    // Try different protocols for the secondary instance
    let protocols = vec![
        ProtocolType::UnixSocket,
        ProtocolType::FileBased,
        ProtocolType::SharedMemory,
    ];
    
    for protocol in protocols {
        println!("Trying to connect with {} protocol...", 
                 match protocol {
                     ProtocolType::UnixSocket => "Unix Socket",
                     ProtocolType::FileBased => "File-based",
                     ProtocolType::SharedMemory => "Shared Memory",
                     _ => "Unknown",
                 });
        
        let mut app = SingleInstanceApp::new(identifier)
            .with_protocol(protocol)
            .with_timeout(5000);
        
        match app.enforce_single_instance().await {
            Ok(true) => {
                println!("Unexpected: Secondary instance became primary");
                process::exit(1);
            }
            Ok(false) => {
                println!("Successfully connected as secondary instance");
                process::exit(0);
            }
            Err(e) => {
                println!("Failed with {}: {}", 
                         match protocol {
                             ProtocolType::UnixSocket => "Unix Socket",
                             ProtocolType::FileBased => "File-based",
                             ProtocolType::SharedMemory => "Shared Memory",
                             _ => "Unknown",
                         }, e);
                continue;
            }
        }
    }
    
    eprintln!("Failed to connect to primary instance with any protocol");
    process::exit(1);
}
