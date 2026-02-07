import { SingleInstanceApp, IpcClient,lib } from "./bun_ffi";

// Protocol enum
enum Protocol {
  UnixSocket = 0,
  NamedPipe = 1,
  SharedMemory = 2,
  FileBased = 3,
  InMemory = 4,
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
      
      // Keep the host running and periodically check for messages from clients
      const messageCheckInterval = setInterval(() => {
        try {
          const msg = app.receive();
          if (!msg) return;
          if (msg.message_type !== "chat") return;
          if (!msg.payload) return;
          const payload = msg.payload as { content: string; from: string; isHost?: boolean };
          if (payload.from !== username) {
            console.log(`\r[CHAT] ${payload.from}: ${payload.content}`);
            process.stdout.write("> ");
          }
        } catch (e) {
          // Ignore parse errors from non-chat messages
        }
      }, 100);
      
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
            clearInterval(messageCheckInterval);
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
        clearInterval(messageCheckInterval);
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
      
      // Establish persistent connection for receiving broadcasts
      client.connect();
      
      // Create readline interface
      const readline = await import("node:readline");
      const rl = readline.createInterface({
        input: process.stdin,
        output: process.stdout
      });
      
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
          if (!msg.payload) return;
          const payload = msg.payload as { content: string; from: string; isHost?: boolean };
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
