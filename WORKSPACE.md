# Workspace Structure Summary

## Overview

This workspace has been configured to separate the core IPC library from its multi-language bindings, following best practices for Rust library distribution.

## Workspace Members

### 1. **`single_instance_app`** (Root Crate)

- **Location**: `./src/`
- **Type**: Library (`lib`)
- **Purpose**: Core Rust IPC library
- **Publishing**: ✅ Publish to crates.io
- **Dependencies**: Minimal (tokio, serde, etc.)

### 2. **`bindings_uniffi`**

- **Location**: `./bindings_uniffi/`
- **Type**: C Dynamic Library (`cdylib`)
- **Purpose**: Multi-language bindings via UniFFI
- **Publishing**: ❌ Do NOT publish to crates.io
- **Supported Languages**: Python, Swift, Kotlin, Ruby
- **Build Output**: `librust_ipc_bindings.{so,dylib,dll}`

### 3. **`ipc_chat_example`**

- **Location**: `./ipc_chat_example/`
- **Type**: Binary
- **Purpose**: Example application demonstrating multi-instance chat
- **Publishing**: ❌ Example only

### 4. **`ipc_wgpu_example`**

- **Location**: `./ipc_wgpu_example/`
- **Type**: Binary
- **Purpose**: Example demonstrating GPU rendering with IPC
- **Publishing**: ❌ Example only

## Why This Structure?

### Separation of Concerns

1. **Core Library Stays Lightweight**
   - Rust users only need `single_instance_app`
   - No UniFFI dependencies unless needed
   - Faster compilation for Rust-only projects

2. **Bindings Are Optional**
   - Only build `bindings_uniffi` when generating language bindings
   - Keeps crates.io package small
   - Allows independent versioning

3. **Examples Don't Pollute**
   - Examples are separate workspace members
   - Won't be included in published crate
   - Can have their own heavy dependencies (like wgpu)

## Publishing Strategy

### To crates.io (Rust Library)

```bash
# Only publish the core library
cd /path/to/RUST_IPC
cargo publish
```

The `Cargo.toml` at the root defines the library. The workspace members are excluded from the published package.

### For Other Languages (Bindings)

**Don't publish bindings to crates.io!** Instead:

1. **Python**: Publish to PyPI using generated wheel

   ```bash
   cd bindings_uniffi
   ./build_bindings.sh python
   # Create Python package and publish to PyPI
   ```

2. **Swift**: Distribute via Swift Package Manager or CocoaPods
3. **Kotlin**: Publish to Maven Central
4. **Ruby**: Publish to RubyGems

## Build Commands

### Build Everything

```bash
cargo build --workspace
```

### Build Only Core Library

```bash
cargo build
# or
cargo build -p single_instance_app
```

### Build Only Bindings

```bash
cargo build -p bindings_uniffi --release
```

### Generate Language Bindings

```bash
cd bindings_uniffi
./build_bindings.sh python kotlin swift
```

## Directory Structure

```
RUST_IPC/
├── Cargo.toml              # Workspace definition + core library
├── Cargo.lock
├── README.md               # Main documentation
├── src/                    # Core library source
│   ├── lib.rs
│   └── communication/
├── bindings_uniffi/        # Multi-language bindings
│   ├── Cargo.toml
│   ├── README.md
│   ├── src/lib.rs
│   ├── build_bindings.sh
│   └── examples/
│       └── python_example.py
├── ipc_chat_example/       # Example: Chat app
│   ├── Cargo.toml
│   └── src/main.rs
├── ipc_wgpu_example/       # Example: GPU rendering
│   ├── Cargo.toml
│   └── src/
└── target/                 # Build artifacts
    └── release/
        └── librust_ipc_bindings.so
```

## Testing

```bash
# Test core library
cargo test

# Test specific member
cargo test -p bindings_uniffi
cargo test -p ipc_chat_example

# Test all workspace members
cargo test --workspace
```

## Continuous Integration

Recommended CI workflow:

1. **Core Library CI**:
   - Build and test `single_instance_app`
   - Run on multiple platforms (Linux, macOS, Windows)
   - Check formatting and lints

2. **Bindings CI**:
   - Build `bindings_uniffi`
   - Generate bindings for all languages
   - Run language-specific tests (Python, etc.)

3. **Examples CI**:
   - Build examples to ensure they compile
   - Optional: Run integration tests

## Migration Notes

If you were using a single crate before:

1. ✅ Core library API remains unchanged
2. ✅ Existing Rust code continues to work
3. ✅ New: Multi-language support via bindings
4. ✅ New: Better separation for publishing

## Next Steps

1. **For Rust Users**: Just use `single_instance_app` from crates.io
2. **For Python Users**: Build bindings and create a PyPI package
3. **For Mobile Developers**: Generate Swift/Kotlin bindings
4. **For Contributors**: See individual README files in each crate
