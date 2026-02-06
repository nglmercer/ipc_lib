use serde::{Deserialize, Serialize};
use single_instance_app::{
    communication::{CommunicationMessage, ProtocolType},
    IpcClient, SingleInstanceApp,
};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::sync::{Arc, Mutex};
use uuid::Uuid;
use winit::{
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
    window::WindowBuilder,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
enum SubtitleAction {
    AddText {
        id: String,
        text: String,
        x: f32,
        y: f32,
        size: f32,
        color: [u8; 3],
    },
    RemoveText {
        id: String,
    },
    Clear,
}

#[allow(dead_code)]
struct SubtitleText {
    text: String,
    x: f32,
    y: f32,
    size: f32,
    color: [u8; 3],
}

fn main() {
    let identifier = "ipc_subtitle_service";
    let texts = Arc::new(Mutex::new(HashMap::<String, SubtitleText>::new()));
    let texts_for_handler = texts.clone();

    // Create a background runtime for IPC
    let rt = tokio::runtime::Runtime::new().unwrap();
    let _guard = rt.enter();

    let mut app = rt.block_on(async {
        SingleInstanceApp::new(identifier)
            .with_protocol(ProtocolType::UnixSocket)
            .on_message(move |msg| {
                if msg.message_type == "subtitle" {
                    if let Ok(action) = serde_json::from_value::<SubtitleAction>(msg.payload) {
                        let mut texts = texts_for_handler.lock().unwrap();
                        match action {
                            SubtitleAction::AddText { id, text, x, y, size, color } => {
                                texts.insert(id, SubtitleText { text, x, y, size, color });
                            }
                            SubtitleAction::RemoveText { id } => {
                                texts.remove(&id);
                            }
                            SubtitleAction::Clear => {
                                texts.clear();
                            }
                        }
                    }
                }
            })
    });

    let is_primary = rt.block_on(app.enforce_single_instance());

    match is_primary {
        Ok(true) => {
            run_renderer(texts);
        }
        Ok(false) => {
            rt.block_on(send_test_message(identifier));
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }
}

async fn send_test_message(identifier: &str) {
    let mut client = IpcClient::new(identifier).unwrap();
    let action = SubtitleAction::AddText {
        id: Uuid::new_v4().to_string(),
        text: "LIGHTWEIGHT RENDERER TEST".to_string(),
        x: 100.0,
        y: 100.0,
        size: 32.0,
        color: [255, 255, 0],
    };

    let msg = CommunicationMessage {
        message_type: "subtitle".to_string(),
        payload: serde_json::to_value(action).unwrap(),
        timestamp: 0,
        source_id: "client".to_string(),
        metadata: serde_json::json!(null),
    };

    if let Err(e) = client.send_message(msg).await {
        eprintln!("Failed to send: {}", e);
    } else {
        println!("✅ Message sent");
    }
}

fn run_renderer(texts: Arc<Mutex<HashMap<String, SubtitleText>>>) {
    let event_loop = EventLoop::new().unwrap();
    let window = WindowBuilder::new()
        .with_title("IPC Subtitles")
        .with_transparent(true)
        .with_decorations(false)
        .with_window_level(winit::window::WindowLevel::AlwaysOnTop)
        // Note: Click-through is platform dependent and often requires raw window handle calls
        // In winit 0.29+ it might be available via platform attributes
        .build(&event_loop)
        .unwrap();

    let window = Arc::new(window);
    let context = softbuffer::Context::new(window.clone()).unwrap();
    let mut surface = softbuffer::Surface::new(&context, window.clone()).unwrap();

    // In a real app, you'd load a font here. 
    // For this "render test" we'll just draw boxes where the text should be to prove it works.
    
    event_loop.set_control_flow(ControlFlow::Poll);
    let _ = event_loop.run(move |event, elwt| {
        match event {
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => elwt.exit(),
            Event::AboutToWait => {
                window.request_redraw();
            }
            Event::WindowEvent {
                event: WindowEvent::RedrawRequested,
                ..
            } => {
                let (width, height) = {
                    let size = window.inner_size();
                    (size.width, size.height)
                };
                
                if width == 0 || height == 0 { return; }
                
                surface.resize(
                    NonZeroU32::new(width).unwrap(),
                    NonZeroU32::new(height).unwrap()
                ).unwrap();

                let mut buffer = surface.buffer_mut().unwrap();
                
                // Clear with transparent (0) or semi-transparent
                buffer.fill(0); 

                let texts = texts.lock().unwrap();
                for t in texts.values() {
                    // Simple box renderer for "lightweight test"
                    // In a full implementation, tiny-skia + fontdue would go here
                    let x = t.x as u32;
                    let y = t.y as u32;
                    let size = t.size as u32;
                    
                    for dy in 0..size {
                        for dx in 0..(size * 2) { // wider box for text placeholder
                            let py = y + dy;
                            let px = x + dx;
                            if px < width && py < height {
                                let color = (255 << 24) | ((t.color[0] as u32) << 16) | ((t.color[1] as u32) << 8) | (t.color[2] as u32);
                                buffer[(py * width + px) as usize] = color;
                            }
                        }
                    }
                }
                
                buffer.present().unwrap();
            }
            _ => (),
        }
    });
}
