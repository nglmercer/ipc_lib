/**
 * Bun FFI Example for Single Instance App IPC Library
 * 
 * This example demonstrates how to use the Rust IPC library from Bun/TypeScript
 * using Bun's native FFI (Foreign Function Interface).
 * 
 * Prerequisites:
 * 1. Build the library: cargo build --release -p bindings_uniffi
 * 2. Install Bun: https://bun.sh
 * 3. Run: bun run bun_ffi_example.ts
 */

import { dlopen, FFIType, type Pointer, ptr, CString } from "bun:ffi";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";

// Get the directory of this file
const __dirname = fileURLToPath(new URL(".", import.meta.url));

// Determine the library path based on the platform
const suffix = process.platform === "darwin" ? "dylib" : process.platform === "win32" ? "dll" : "so";
const libPath = resolve(__dirname, "../../target/release", `librust_ipc_bindings.${suffix}`);

console.log(`📚 Loading library from: ${libPath}`);

// Define the FFI interface matching our C ABI
const lib = dlopen(libPath, {
  // Create a new app instance
  ipc_app_new: {
    args: [FFIType.cstring],
    returns: FFIType.ptr,
  },
  
  // Enforce single instance (returns 1=primary, 0=secondary, -1=error)
  ipc_app_enforce_single_instance: {
    args: [FFIType.ptr],
    returns: FFIType.i32,
  },
  
  // Set protocol (0=UnixSocket, 1=NamedPipe, 2=SharedMemory, 3=FileBased, 4=InMemory)
  ipc_app_set_protocol: {
    args: [FFIType.ptr, FFIType.i32],
    returns: FFIType.i32,
  },
  
  // Broadcast a message
  ipc_app_broadcast: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring],
    returns: FFIType.i32,
  },
  
  // Get last error message
  ipc_app_get_last_error: {
    args: [],
    returns: FFIType.cstring,
  },
  
  // Free the app handle
  ipc_app_free: {
    args: [FFIType.ptr],
    returns: FFIType.void,
  },
  
  // Free error string
  ipc_app_free_error: {
    args: [FFIType.ptr],
    returns: FFIType.void,
  },
});

console.log("✅ Library loaded successfully!");

// Protocol enum
enum Protocol {
  UnixSocket = 0,
  NamedPipe = 1,
  SharedMemory = 2,
  FileBased = 3,
  InMemory = 4,
}

// Wrapper class for easier usage
class SingleInstanceApp {
  private handle: Pointer | null;
  
  constructor(identifier: string) {
    const identifierCStr = Buffer.from(identifier + "\0", "utf-8");
    this.handle = lib.symbols.ipc_app_new(identifierCStr as unknown as CString);
    
    if (this.handle === null || this.handle === 0) {
      throw new Error("Failed to create app instance");
    }
  }
  
  setProtocol(protocol: Protocol): void {
    const result = lib.symbols.ipc_app_set_protocol(this.handle, protocol);
    if (result !== 0) {
      throw new Error("Failed to set protocol");
    }
  }
  
  enforceSingleInstance(): "primary" | "secondary" {
    const result = lib.symbols.ipc_app_enforce_single_instance(this.handle);
    
    if (result === 1) {
      return "primary";
    } else if (result === 0) {
      return "secondary";
    } else {
      const errorPtr = lib.symbols.ipc_app_get_last_error();
      let error = "Unknown error";
      if (errorPtr && errorPtr !== null) {
        const errorStr = new CString(errorPtr as unknown as Pointer);
        error = errorStr.toString();
        lib.symbols.ipc_app_free_error(errorPtr as unknown as Pointer);
      }
      throw new Error(`Failed to enforce single instance: ${error}`);
    }
  }
  
  broadcast(messageType: string, payload: object): void {
    const payloadJson = JSON.stringify(payload);
    const messageTypeCStr = Buffer.from(messageType + "\0", "utf-8");
    const payloadCStr = Buffer.from(payloadJson + "\0", "utf-8");
    const result = lib.symbols.ipc_app_broadcast(
      this.handle,
      messageTypeCStr as unknown as CString,
      payloadCStr as unknown as CString
    );
    
    if (result !== 0) {
      throw new Error("Failed to broadcast message");
    }
  }
  
  close(): void {
    if (this.handle !== null && this.handle !== 0) {
      lib.symbols.ipc_app_free(this.handle);
      this.handle = null;
    }
  }
}

// Example usage
async function main() {
  console.log("\n🚀 Starting Single Instance App Example (Bun FFI)\n");
  
  try {
    const app = new SingleInstanceApp("bun-ipc-example");
    
    // Use Unix sockets (default on Unix-like systems)
    app.setProtocol(Protocol.UnixSocket);
    
    const instanceType = app.enforceSingleInstance();
    
    if (instanceType === "primary") {
      console.log("🏠 I am the PRIMARY instance!");
      console.log("   Waiting for messages from other instances...");
      console.log("   Try running this script again in another terminal!");
      console.log("   Press Ctrl+C to exit\n");
      
      // Keep the primary instance running
      let counter = 0;
      const interval = setInterval(() => {
        counter++;
        if (counter % 5 === 0) {
          console.log(`💓 Heartbeat ${counter}s - Still running as primary...`);
        }
      }, 1000);
      
      // Handle cleanup on exit
      process.on("SIGINT", () => {
        console.log("\n\n👋 Shutting down primary instance...");
        clearInterval(interval);
        app.close();
        lib.close();
        process.exit(0);
      });
      
    } else {
      console.log("👥 I am a SECONDARY instance!");
      console.log("   Connected to existing primary instance");
      console.log("   The primary instance received our connection message\n");
      
      // Cleanup and exit
      app.close();
      lib.close();
    }
    
  } catch (error) {
    console.error("❌ Error:", error);
    lib.close();
    process.exit(1);
  }
}

// Run the example
main();
