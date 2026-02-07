"""
Example usage of the Rust IPC library from Python via UniFFI bindings.

Before running this example:
1. Build the bindings: cd bindings_uniffi && ./build_bindings.sh python
2. Copy the generated .so/.dylib/.dll and .py files to this directory
3. Install dependencies: pip install asyncio
"""

import asyncio
import sys
from rust_ipc_bindings import SingleInstanceApp, ProtocolType, CommunicationMessage

class MyMessageHandler:
    """Custom message handler that responds to incoming messages"""
    
    def on_message(self, message: CommunicationMessage):
        print(f"📨 Received message:")
        print(f"   Type: {message.message_type}")
        print(f"   From: {message.source_id}")
        print(f"   Payload: {message.payload_json}")
        
        # Return a response
        return CommunicationMessage(
            id="response-id",
            message_type="response",
            payload_json='{"status": "received"}',
            timestamp=message.timestamp,
            source_id="python-handler",
            reply_to=message.id,
            metadata_json="{}"
        )

async def main():
    print("🚀 Starting Single Instance App Example (Python)")
    
    # Create the app instance
    app = SingleInstanceApp("python-ipc-example")
    
    # Configure to use Unix sockets (default)
    await app.with_protocol(ProtocolType.UnixSocket)
    
    # Set up message handler
    handler = MyMessageHandler()
    await app.on_message(handler)
    
    # Try to enforce single instance
    try:
        is_primary = await app.enforce_single_instance()
        
        if is_primary:
            print("🏠 I am the PRIMARY instance!")
            print("   Waiting for messages from other instances...")
            print("   Press Ctrl+C to exit")
            
            # Keep the primary instance running
            try:
                while True:
                    await asyncio.sleep(1)
                    
                    # Optionally broadcast a message every 5 seconds
                    # await app.broadcast("heartbeat", '{"status": "alive"}')
                    
            except KeyboardInterrupt:
                print("\n👋 Shutting down primary instance")
        else:
            print("👥 I am a SECONDARY instance!")
            print("   Connected to existing primary instance")
            print("   The primary instance received our connection message")
            
    except Exception as e:
        print(f"❌ Error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    asyncio.run(main())
