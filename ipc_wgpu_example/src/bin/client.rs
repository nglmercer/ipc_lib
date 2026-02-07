use single_instance_app::{
    communication::CommunicationMessage, communication::ProtocolType, communication::SerializationFormat, IpcClient,
};
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let identifier = "wgpu_render_server";
    println!("🔗 Client starting for: {}...", identifier);

    let client = IpcClient::new(identifier)?
        .with_protocol(ProtocolType::UnixSocket)
        .with_serialization_format(SerializationFormat::MsgPack);

    let start_time = Instant::now();
    let width = 512;
    let height = 512;
    // Pre-allocate buffer
    let mut pixels = vec![0u8; width * height * 4];

    // Outer loop for automatic reconnection
    loop {
        println!("🔗 Connecting to wgpu Render Server...");
        
        let messenger = loop {
            match client.connect_messenger().await {
                Ok(m) => break m,
                Err(e) => {
                    // Only log periodically or on first attempt to avoid spamming? 
                    // For now, keep existing behavior but maybe slightly less frequent
                    println!("⏳ Waiting for server... ({})", e);
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            }
        };
        println!("✅ Connected! Sending frames...");

        // Inner loop for sending frames
        loop {
            let elapsed = start_time.elapsed().as_secs_f32();

            // Generate an animated pattern: a moving square
            let square_size = 100;
            let square_x = ((elapsed.sin() * 0.5 + 0.5) * (width - square_size) as f32) as usize;
            let square_y = ((elapsed.cos() * 0.5 + 0.5) * (height - square_size) as f32) as usize;

            // Background: dynamic color
            let bg_r = ((elapsed * 0.5).sin() * 50.0 + 50.0) as u8;
            let bg_g = ((elapsed * 0.7).cos() * 50.0 + 50.0) as u8;
            let bg_b = ((elapsed * 0.9).sin() * 50.0 + 50.0) as u8;

            for y in 0..height {
                for x in 0..width {
                    let offset = (y * width + x) * 4;

                    if x >= square_x
                        && x < square_x + square_size
                        && y >= square_y
                        && y < square_y + square_size
                    {
                        // Square color
                        pixels[offset] = 255; // R
                        pixels[offset + 1] = 255; // G
                        pixels[offset + 2] = 0; // B
                        pixels[offset + 3] = 255; // A
                    } else {
                        // Background color
                        pixels[offset] = bg_r;
                        pixels[offset + 1] = bg_g;
                        pixels[offset + 2] = bg_b;
                        pixels[offset + 3] = 255;
                    }
                }
            }

            // Encode pixels as Base64 string to avoid JSON overhead
            use base64::{engine::general_purpose, Engine as _};
            let pixels_b64 = general_purpose::STANDARD.encode(&pixels);

            let msg = CommunicationMessage::new(
                "render_frame",
                serde_json::json!({
                    "pixels_b64": pixels_b64
                }),
            );

            let msg_id = msg.id.clone();
            // println!("📤 Sending frame... ID: {}", msg_id);

            // Send the frame.
            match messenger.request(msg).await {
                Ok(_) => {
                    // println!("✅ Frame {} sent successfully", msg_id);
                    // Throttling to approx 60fps
                    tokio::time::sleep(Duration::from_millis(16)).await;
                }
                Err(e) => {
                    eprintln!("❌ Failed to send frame {}: {}", msg_id, e);
                    eprintln!("⚠️ Connection lost. Reconnecting in 1 second...");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    break; // Break logic to reconnect
                }
            }
        }
    }
}
