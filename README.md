# Single Instance App - IPC Library

A robust Rust library for single-instance application enforcement with multiple IPC (Inter-Process Communication) protocols.

## 🚀 Features

- **Single Instance Enforcement**: Ensure only one instance of your application runs at a time
- **Multiple IPC Protocols**: Unix sockets, shared memory, file-based, and in-memory communication
- **Async/Await Support**: Built on Tokio for modern async Rust applications
- **Fallback Mechanisms**: Automatic protocol fallback for reliability
- **Cross-Platform**: Support for Unix-like systems (Linux, macOS) and Windows
- **Multi-Language Bindings**: Use from Python, Node.js, Swift, Kotlin, and more via UniFFI

## 📦 Project Structure

This is a Cargo workspace with the following crates:

```
RUST_IPC/
├── src/                      # Core library (single_instance_app)
│   ├── lib.rs               # Main library interface
│   └── communication/       # IPC protocol implementations
├── bindings_uniffi/         # Multi-language bindings (Python, etc.)
├── ipc_chat_example/        # Example: Multi-instance chat application
└── ipc_wgpu_example/        # Example: GPU rendering with IPC
```

### Core Library (`single_instance_app`)

The main Rust library that can be used directly in Rust projects.

**Add to your `Cargo.toml`:**

```toml
[dependencies]
single_instance_app = "0.1.0"
```

**Basic Usage:**

```rust
use single_instance_app::{SingleInstanceApp, ProtocolType};

#[tokio::main]
async fn main() {
    // The default protocol is platform-specific:
    // - Unix/Linux/macOS: UnixSocket (fastest)
    // - Windows: FileBased (cross-platform)
    let mut app = SingleInstanceApp::new("my-unique-app-id")
        // .with_protocol(ProtocolType::UnixSocket) // Optional: specify protocol explicitly
        .on_message(|msg| {
            println!("Received: {:?}", msg);
            None // Or return Some(response_message)
        });

    match app.enforce_single_instance().await {
        Ok(true) => println!("🏠 I am the primary instance!"),
        Ok(false) => println!("👋 Connected to existing instance"),
        Err(e) => eprintln!("❌ Error: {}", e),
    }
}
```

### Multi-Language Bindings (`bindings_uniffi`)

Use the library from **Python, Node.js, Swift, Kotlin, Ruby**, and more via [UniFFI](https://github.com/mozilla/uniffi-rs).

**Why a separate crate?**

- Keeps the core library lightweight for Rust-only users
- Avoids heavy build dependencies when publishing to crates.io
- Allows independent versioning of bindings

**Build bindings:**

```bash
# Build the shared library
cargo build --release -p bindings_uniffi

# Generate Python bindings
cargo run --bin uniffi-bindgen generate \
  --library target/release/librust_ipc_bindings.so \
  --language python \
  --out-dir bindings/python
```

**Python Example:**

```python
from rust_ipc_bindings import SingleInstanceApp, ProtocolType

async def main():
    app = SingleInstanceApp("my-app")
    is_primary = await app.enforce_single_instance()

    if is_primary:
        print("🏠 Primary instance!")
    else:
        print("👋 Secondary instance")
```

See [`bindings_uniffi/README.md`](bindings_uniffi/README.md) for detailed instructions.

## 🎯 Available IPC Protocols

| Protocol         | Platform         | Speed            | Use Case                         |
| ---------------- | ---------------- | ---------------- | -------------------------------- |
| **UnixSocket**   | Unix/Linux/macOS | ⚡⚡⚡ Fast      | Default on Unix, best performance |
| **SharedMemory** | Unix/Linux/macOS | ⚡⚡⚡ Fastest   | High-throughput data transfer    |
| **FileBased**    | All              | ⚡ Slower        | Cross-platform, reliable fallback |
| **InMemory**     | All              | ⚡⚡⚡⚡ Instant | Testing only                     |
| **NamedPipe**    | Windows          | ⚡⚡ Fast        | Not yet implemented              |

## 📚 Examples

### Chat Application (`ipc_chat_example`)

A multi-instance chat where the first instance becomes the host and subsequent instances connect as clients.

```bash
cd ipc_chat_example
cargo run
```

### GPU Rendering (`ipc_wgpu_example`)

Demonstrates sharing GPU rendering state between instances.

```bash
cd ipc_wgpu_example
cargo run --bin wgpu_example  # Primary instance
cargo run --bin client        # Secondary instance
```

## 🔧 Configuration Options

```rust
let app = SingleInstanceApp::new("app-id")
    .with_protocol(ProtocolType::UnixSocket)
    .with_timeout(5000)  // 5 seconds
    .without_fallback()  // Disable automatic fallback
    .with_fallback_protocols(vec![
        ProtocolType::FileBased,
        ProtocolType::SharedMemory,
    ]);
```

## 🧪 Testing

```bash
# Test core library
cargo test

# Test specific protocol
cargo test --features test-unix-socket

# Test bindings
cargo test -p bindings_uniffi
```

## 📖 Documentation

Generate and view the full API documentation:

```bash
cargo doc --open
```

## 🤝 Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## 📄 License

This project is licensed under the MIT License - see the LICENSE file for details.

## 🙏 Acknowledgments

- Built with [Tokio](https://tokio.rs/) for async runtime
- Multi-language bindings powered by [UniFFI](https://github.com/mozilla/uniffi-rs)
- Inspired by single-instance patterns from various desktop applications

## 🔗 Related Projects

For **Node.js-specific** bindings with better performance and TypeScript support, consider using [napi-rs](https://napi.rs/) instead of UniFFI.

For **Python-specific** bindings with better integration, consider using [PyO3](https://pyo3.rs/) + [Maturin](https://www.maturin.rs/).

UniFFI is recommended when you need to support **multiple languages** with a single codebase.
