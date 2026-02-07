# UniFFI Bindings for Single Instance App IPC Library

This crate provides **Multi-Language Bindings** for the Single Instance App IPC library using [UniFFI](https://github.com/mozilla/uniffi-rs).

With these bindings, you can use the Rust IPC library from:

- 🐍 **Python**
- 📱 **Swift** (iOS/macOS)
- 🤖 **Kotlin** (Android/JVM)
- 💎 **Ruby**
- ⚡ **Bun/Node.js** (via FFI - see [Bun FFI Example](examples/README_BUN.md))

## 🎯 Why a Separate Crate?

The bindings are in a separate crate to:

1. **Keep the core library lightweight** - Rust-only users don't need UniFFI dependencies
2. **Avoid bloating crates.io** - Build artifacts and bindings don't need to be published
3. **Independent versioning** - Bindings can evolve separately from the core library

## 📋 Prerequisites

### For All Languages

```bash
# Install uniffi-bindgen (if not using cargo integration)
cargo install uniffi-bindgen
```

### Language-Specific

**Python:**

```bash
pip install cffi  # Required for loading the library
```

**Kotlin:**

- JDK 11 or higher
- Gradle or Maven

**Swift:**

- Xcode (macOS)
- Swift 5.5+

## 🔨 Building Bindings

### Quick Start (Using the Build Script)

```bash
# Generate Python bindings
./build_bindings.sh python

# Generate multiple languages at once
./build_bindings.sh python kotlin swift
```

The script will:

1. Build the Rust library in release mode
2. Generate bindings for the specified language(s)
3. Place them in `../bindings/<language>/`

### Manual Build

```bash
# 1. Build the library
cargo build --release -p bindings_uniffi

# 2. Generate bindings (example for Python)
cargo run --bin uniffi-bindgen generate \
  --library ../target/release/librust_ipc_bindings.so \
  --language python \
  --out-dir ../bindings/python
```

**Note:** Replace `.so` with:

- `.dylib` on macOS
- `.dll` on Windows

## 🐍 Python Usage

### Installation

After building the bindings:

```bash
# Copy the generated files to your Python project
cp ../bindings/python/rust_ipc_bindings.py your_project/
cp ../target/release/librust_ipc_bindings.so your_project/
```

### Example

```python
import asyncio
from rust_ipc_bindings import SingleInstanceApp, ProtocolType

async def main():
    app = SingleInstanceApp("my-app-id")
    await app.with_protocol(ProtocolType.UnixSocket)

    is_primary = await app.enforce_single_instance()

    if is_primary:
        print("🏠 I am the primary instance!")
        # Keep running to handle messages
        await asyncio.sleep(3600)
    else:
        print("👥 Connected to existing instance")

asyncio.run(main())
```

See [`examples/python_example.py`](examples/python_example.py) for a complete example with message handling.

## 📱 Swift Usage (iOS/macOS)

### Integration

1. Add the generated `.swift` file to your Xcode project
2. Add the `.dylib` to your app bundle or framework

### Example

```swift
import Foundation

let app = SingleInstanceApp(identifier: "my-app-id")

Task {
    do {
        try await app.withProtocol(protocol: .unixSocket)
        let isPrimary = try await app.enforceSingleInstance()

        if isPrimary {
            print("🏠 Primary instance")
        } else {
            print("👥 Secondary instance")
        }
    } catch {
        print("Error: \(error)")
    }
}
```

## 🤖 Kotlin Usage (Android/JVM)

### Integration

1. Add the generated `.kt` files to your project
2. Include the native library in your JNI libs

### Example

```kotlin
import kotlinx.coroutines.runBlocking

fun main() = runBlocking {
    val app = SingleInstanceApp("my-app-id")
    app.withProtocol(ProtocolType.UNIX_SOCKET)

    val isPrimary = app.enforceSingleInstance()

    if (isPrimary) {
        println("🏠 Primary instance")
    } else {
        println("👥 Secondary instance")
    }
}
```

## 🧪 Testing

```bash
# Test the bindings crate
cargo test -p bindings_uniffi

# Build and verify all bindings
./build_bindings.sh python kotlin swift ruby
```

## 📊 API Reference

### Types

**`ProtocolType` (Enum)**

- `UnixSocket` - Unix domain sockets (Linux/macOS)
- `NamedPipe` - Named pipes (Windows)
- `SharedMemory` - Shared memory
- `FileBased` - File-based communication
- `InMemory` - In-memory (testing only)

**`CommunicationMessage` (Record)**

- `id: String` - Unique message ID
- `message_type: String` - Type of message
- `payload_json: String` - JSON payload
- `timestamp: u64` - Unix timestamp in milliseconds
- `source_id: String` - Source identifier
- `reply_to: Option<String>` - ID of message being replied to
- `metadata_json: String` - Additional metadata

**`MessageHandler` (Callback Interface)**

- `on_message(message: CommunicationMessage) -> Option<CommunicationMessage>`

### Main Object: `SingleInstanceApp`

**Constructor:**

```
SingleInstanceApp(identifier: String)
```

**Methods:**

- `async with_protocol(protocol: ProtocolType)` - Set the IPC protocol
- `async on_message(handler: MessageHandler)` - Set message handler
- `async enforce_single_instance() -> bool` - Enforce single instance (returns true if primary)
- `async broadcast(message_type: String, payload_json: String)` - Broadcast to all clients

## ⚠️ Known Limitations

1. **Async Support**: UniFFI's async support is still evolving. Some languages may require additional runtime setup.
2. **Node.js**: Not directly supported by UniFFI. For Node.js, use [napi-rs](https://napi.rs/) instead.
3. **Callbacks**: Callback interfaces work differently across languages. See UniFFI docs for details.

## 🔗 Alternative Approaches

### For Node.js Only

Use **napi-rs** for better performance and TypeScript support:

```bash
cargo install napi-cli
```

### For Python Only

Use **PyO3** + **Maturin** for better Python integration:

```bash
pip install maturin
```

### When to Use UniFFI

- You need **multiple languages** (mobile + desktop + scripting)
- You want **one codebase** for all bindings
- You're okay with the async limitations

## 📚 Resources

- [UniFFI Documentation](https://mozilla.github.io/uniffi-rs/)
- [UniFFI Examples](https://github.com/mozilla/uniffi-rs/tree/main/examples)
- [Core Library README](../README.md)

## 🤝 Contributing

Contributions are welcome! Please test bindings in your target language before submitting PRs.
