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
  // ===== App Functions =====
  
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

  // Free a string
  ipc_app_free_string: {
    args: [FFIType.ptr],
    returns: FFIType.void,
  },

  // ===== Client Functions =====

  // Create a new client instance
  ipc_client_new: {
    args: [FFIType.cstring],
    returns: FFIType.ptr,
  },

  // Set protocol for client
  ipc_client_set_protocol: {
    args: [FFIType.ptr, FFIType.i32],
    returns: FFIType.i32,
  },

  // Send a message from client
  ipc_client_send: {
    args: [FFIType.ptr, FFIType.cstring, FFIType.cstring],
    returns: FFIType.i32,
  },

  // Receive a message (non-blocking)
  ipc_client_receive: {
    args: [FFIType.ptr],
    returns: FFIType.ptr,
  },

  // Ping server to check connection
  ipc_client_ping: {
    args: [FFIType.ptr],
    returns: FFIType.i32,
  },

  // Disconnect and cleanup persistent connection
  ipc_client_disconnect: {
    args: [FFIType.ptr],
    returns: FFIType.i32,
  },

  // Free client handle
  ipc_client_free: {
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

// Message interface matching Rust CommunicationMessage
interface CommunicationMessage {
  id: string;
  message_type: string;
  payload_json: string;
  timestamp: number;
  source_id: string;
  reply_to?: string;
}

// Wrapper class for SingleInstanceApp (Primary instance)
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

// Wrapper class for IPC Client (Secondary instance)
class IpcClient {
  private handle: Pointer | null;
  
  constructor(identifier: string) {
    const identifierCStr = Buffer.from(identifier + "\0", "utf-8");
    this.handle = lib.symbols.ipc_client_new(identifierCStr as unknown as CString);
    
    if (this.handle === null || this.handle === 0) {
      throw new Error("Failed to create client instance");
    }
  }
  
  setProtocol(protocol: Protocol): void {
    const result = lib.symbols.ipc_client_set_protocol(this.handle, protocol);
    if (result !== 0) {
      throw new Error("Failed to set client protocol");
    }
  }
  
  ping(): boolean {
    const result = lib.symbols.ipc_client_ping(this.handle);
    return result === 1;
  }
  
  send(messageType: string, payload: object): boolean {
    const payloadJson = JSON.stringify(payload);
    const messageTypeCStr = Buffer.from(messageType + "\0", "utf-8");
    const payloadCStr = Buffer.from(payloadJson + "\0", "utf-8");
    const result = lib.symbols.ipc_client_send(
      this.handle,
      messageTypeCStr as unknown as CString,
      payloadCStr as unknown as CString
    );
    return result === 0;
  }
  
  receive(): CommunicationMessage | null {
    const resultPtr = lib.symbols.ipc_client_receive(this.handle);
    
    if (resultPtr === null || resultPtr === 0) {
      return null;
    }
    
    try {
      const resultStr = new CString(resultPtr as unknown as Pointer);
      const jsonStr = resultStr.toString();
      lib.symbols.ipc_app_free_string(resultPtr as unknown as Pointer);
      return JSON.parse(jsonStr) as CommunicationMessage;
    } catch (e) {
      return null;
    }
  }
  
  close(): void {
    if (this.handle !== null && this.handle !== 0) {
      lib.symbols.ipc_client_disconnect(this.handle);
      lib.symbols.ipc_client_free(this.handle);
      this.handle = null;
    }
  }
}

// Helper function to get username
function getUsername(): string {
  const args = process.argv.slice(2);
  const usernameArg = args.find(arg => !arg.startsWith("--") && arg !== "chat");
  return usernameArg || `User${process.pid % 1000}`;
}



// Example usage - Chat Demo
async function runChatDemo() {
  console.log("\n💬 IPC Chat Application (Bun FFI)\n");
  console.log("═══════════════════════════════════════════\n");
  
  const identifier = "bun-chat-example";
  const username = getUsername();
  try {
    const app = new SingleInstanceApp(identifier);
    app.setProtocol(Protocol.UnixSocket);
    
    const instanceType = app.enforceSingleInstance();
    
    if (instanceType === "primary") {
      // ===== PRIMARY INSTANCE (HOST) =====
      console.log(`🏠 You are the HOST! (User: ${username})`);
      console.log("═══════════════════════════════════════════");
      console.log("Chat Commands:");
      console.log("  <message>   - Broadcast message to all clients");
      console.log("  /quit       - Exit chat");
      console.log("═══════════════════════════════════════════\n");
      
      // Keep the host running and periodically check for messages
      let counter = 0;
      const heartbeatInterval = setInterval(() => {
        counter++;
      }, 5000);
      
      // Handle input
      const readline = await import("node:readline");
      const rl = readline.createInterface({
        input: process.stdin,
        output: process.stdout
      });
      
      const askInput = () => {
        rl.question("> ", async (input) => {
          if (input.trim() === "/quit" || input.trim() === "/exit") {
            console.log("👋 Exiting chat...");
            clearInterval(heartbeatInterval);
            rl.close();
            app.close();
            lib.close();
            process.exit(0);
          }
          
          if (input.trim()) {
            app.broadcast("chat", { content: input.trim(), from: username, isHost: true });
            console.log(`📤 You: ${input.trim()}`);
          }
          
          askInput();
        });
      };
      
      askInput();
      
      // Handle cleanup on exit
      process.on("SIGINT", () => {
        console.log("\n👋 Shutting down...");
        clearInterval(heartbeatInterval);
        rl.close();
        app.close();
        lib.close();
        process.exit(0);
      });
      
    } else {
      // ===== SECONDARY INSTANCE (CLIENT) =====
      console.log(`💬 You are a CLIENT! (User: ${username})`);
      console.log("═══════════════════════════════════════════");
      console.log("Chat Commands:");
      console.log("  <message>   - Send message to host");
      console.log("  /quit       - Exit chat");
      console.log("═══════════════════════════════════════════\n");
      
      const client = new IpcClient(identifier);
      client.setProtocol(Protocol.UnixSocket);
      
      // Check if server is available
      if (!client.ping()) {
        console.log("❌ Cannot connect to chat server");
        client.close();
        app.close();
        lib.close();
        return;
      }
      
      console.log("✅ Connected to chat server!\n");
      
      // Create readline interface
      const readline = await import("node:readline");
      const rl = readline.createInterface({
        input: process.stdin,
        output: process.stdout
      });
      
      let isFirstMessage = true;
      
      const askInput = () => {
        rl.question("> ", async (input) => {
          if (input.trim() === "/quit" || input.trim() === "/exit") {
            console.log("👋 Exiting chat...");
            rl.close();
            client.close();
            app.close();
            lib.close();
            process.exit(0);
          }
          
          if (input.trim()) {
            if (client.send("chat", { content: input.trim(), from: username })) {
              console.log(`📤 Sent: ${input.trim()}`);
            } else {
              console.log("❌ Failed to send message");
            }
          }
          
          askInput();
        });
      };
      
      askInput();
      
      // Periodically check for incoming messages
      const messageCheckInterval = setInterval(() => {
        try {
          const msg = client.receive();
          if (!msg) return;
          if (msg.message_type !== "chat") return;
          if (!msg.payload_json) return;
          const payload = JSON.parse(msg.payload_json);
          if (payload.from !== username) {
            console.log(`\r[CHAT] ${payload.from}: ${payload.content}`);
            process.stdout.write("> ");
          }
        } catch (e) {
          // Ignore parse errors from non-chat messages
        }
      }, 100);
      
      // Handle cleanup on exit
      process.on("SIGINT", () => {
        console.log("\n👋 Shutting down...");
        clearInterval(messageCheckInterval);
        rl.close();
        client.close();
        app.close();
        lib.close();
        process.exit(0);
      });
    }
    
  } catch (error) {
    console.error("❌ Error:", error);
    lib.close();
    process.exit(1);
  }
}

runChatDemo();
