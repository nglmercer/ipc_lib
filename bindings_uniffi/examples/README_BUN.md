# Bun FFI Example

This example demonstrates how to use the Rust IPC library from **Bun** using its native FFI (Foreign Function Interface).

## Why Bun FFI?

Bun FFI is **2-6x faster** than Node-API and provides direct access to native libraries without needing to compile Node.js addons.

### Comparison

| Approach    | Speed          | TypeScript Support | Ease of Use | Best For          |
| ----------- | -------------- | ------------------ | ----------- | ----------------- |
| **Bun FFI** | ⚡⚡⚡ Fastest | ✅ Good            | 🟡 Medium   | Bun-specific apps |
| **NAPI-RS** | ⚡⚡ Fast      | ✅✅ Excellent     | ✅ Easy     | Node.js + Bun     |
| **UniFFI**  | ⚡ Slower      | ❌ Manual          | 🟡 Medium   | Multi-language    |

## Prerequisites

1. **Install Bun**:

   ```bash
   curl -fsSL https://bun.sh/install | bash
   ```

2. **Build the Rust library**:
   ```bash
   cd ../..
   cargo build --release -p bindings_uniffi
   ```

## Running the Example

```bash
# From this directory
bun run bun_ffi_example.ts
```

### Testing Single Instance

Open **two terminals** and run the example in both:

**Terminal 1:**

```bash
bun run bun_ffi_example.ts
# Output: 🏠 I am the PRIMARY instance!
```

**Terminal 2:**

```bash
bun run bun_ffi_example.ts
# Output: 👥 I am a SECONDARY instance!
```

## How It Works

### 1. C ABI Layer

The Rust library exposes C-compatible functions in `src/c_abi.rs`:

```rust
#[no_mangle]
pub extern "C" fn ipc_app_new(identifier: *const c_char) -> *mut AppHandle;

#[no_mangle]
pub extern "C" fn ipc_app_enforce_single_instance(handle: *mut AppHandle) -> i32;
```

### 2. Bun FFI Bindings

The TypeScript code uses `dlopen` to load the shared library:

```typescript
const lib = dlopen(libPath, {
  ipc_app_new: {
    args: [FFIType.cstring],
    returns: FFIType.ptr,
  },
  // ... more functions
});
```

### 3. TypeScript Wrapper

A wrapper class provides a clean API:

```typescript
class SingleInstanceApp {
  constructor(identifier: string) {
    /* ... */
  }
  enforceSingleInstance(): "primary" | "secondary" {
    /* ... */
  }
  broadcast(messageType: string, payload: object): void {
    /* ... */
  }
}
```

## API Reference

### `SingleInstanceApp`

#### Constructor

```typescript
new SingleInstanceApp(identifier: string)
```

Creates a new app instance with the given identifier.

#### Methods

**`setProtocol(protocol: Protocol): void`**

Set the IPC protocol to use:

- `Protocol.UnixSocket` - Unix domain sockets (default, Linux/macOS)
- `Protocol.NamedPipe` - Named pipes (Windows)
- `Protocol.SharedMemory` - Shared memory
- `Protocol.FileBased` - File-based communication
- `Protocol.InMemory` - In-memory (testing only)

**`enforceSingleInstance(): "primary" | "secondary"`**

Enforce single instance. Returns:

- `"primary"` - This is the first instance
- `"secondary"` - Another instance is already running

**`broadcast(messageType: string, payload: object): void`**

Broadcast a message to all connected clients (primary instance only).

**`close(): void`**

Free resources and close the app instance.

## Performance Tips

1. **Reuse instances**: Don't create new `SingleInstanceApp` instances repeatedly
2. **Batch operations**: Group multiple operations together when possible
3. **Use appropriate protocols**: Unix sockets are fastest on Unix-like systems

## Troubleshooting

### Library not found

```
Error: Could not open library
```

**Solution**: Make sure you've built the library:

```bash
cargo build --release -p bindings_uniffi
```

### Permission denied

```
Error: Permission denied
```

**Solution**: Check that `/tmp` is writable and you have permissions to create Unix sockets.

### Already running

If you see "Failed to enforce single instance", another instance might be stuck. Clean up:

```bash
rm /tmp/bun-ipc-example.sock
rm /tmp/bun-ipc-example.pid
```

## Limitations

1. **Async**: The C ABI uses blocking calls. For true async, use NAPI-RS instead.
2. **Callbacks**: Message handlers are not yet implemented in the C ABI.
3. **Error handling**: Error messages are basic. Check Rust logs for details.

## Next Steps

- For **production Node.js/Bun apps**, consider using NAPI-RS instead
- For **multiple languages**, use the UniFFI bindings
- For **maximum control**, use the Rust library directly

## Resources

- [Bun FFI Documentation](https://bun.sh/docs/api/ffi)
- [Main Project README](../../README.md)
- [UniFFI Bindings](../README.md)
