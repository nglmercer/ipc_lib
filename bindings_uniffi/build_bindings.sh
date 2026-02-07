#!/usr/bin/env bash
# Build script for generating UniFFI bindings

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$SCRIPT_DIR/.."
BINDINGS_DIR="$PROJECT_ROOT/bindings"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}🔨 Building UniFFI bindings...${NC}"

# Build the library
echo -e "${YELLOW}Building Rust library...${NC}"
cargo build --release -p bindings_uniffi

# Determine library extension based on OS
if [[ "$OSTYPE" == "darwin"* ]]; then
    LIB_EXT="dylib"
elif [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
    LIB_EXT="dll"
else
    LIB_EXT="so"
fi

LIB_PATH="$PROJECT_ROOT/target/release/librust_ipc_bindings.$LIB_EXT"

if [ ! -f "$LIB_PATH" ]; then
    echo -e "${RED}❌ Library not found at: $LIB_PATH${NC}"
    exit 1
fi

echo -e "${GREEN}✅ Library built successfully${NC}"

# Function to generate bindings for a language
generate_bindings() {
    local lang=$1
    local out_dir="$BINDINGS_DIR/$lang"
    
    echo -e "${YELLOW}Generating $lang bindings...${NC}"
    mkdir -p "$out_dir"
    
    cargo run --bin uniffi-bindgen generate \
        --library "$LIB_PATH" \
        --language "$lang" \
        --out-dir "$out_dir"
    
    echo -e "${GREEN}✅ $lang bindings generated at: $out_dir${NC}"
}

# Parse command line arguments
if [ $# -eq 0 ]; then
    echo "Usage: $0 <language> [language...]"
    echo "Available languages: python, kotlin, swift, ruby"
    echo "Example: $0 python kotlin"
    exit 1
fi

# Generate bindings for each requested language
for lang in "$@"; do
    case $lang in
        python|kotlin|swift|ruby)
            generate_bindings "$lang"
            ;;
        *)
            echo -e "${RED}❌ Unknown language: $lang${NC}"
            echo "Available languages: python, kotlin, swift, ruby"
            exit 1
            ;;
    esac
done

echo -e "${GREEN}🎉 All bindings generated successfully!${NC}"
