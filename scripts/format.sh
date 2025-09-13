#!/usr/bin/env bash
# Format all code (Rust and frontend)
# Automatically fixes formatting issues

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Function to print colored output
print_status() {
    echo -e "${GREEN}[FORMAT]${NC} $1"
}

print_error() {
    echo -e "${RED}[FORMAT ERROR]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[FORMAT WARNING]${NC} $1"
}

print_info() {
    echo -e "${BLUE}[FORMAT]${NC} $1"
}

# Change to project root
cd "$PROJECT_ROOT"

# Track if any formatting was done
FORMATTED=0

# Format Rust code
print_info "Formatting Rust code..."
if cargo fmt --all; then
    print_status "✅ Rust code formatted successfully"

    # Check if any files were modified
    if git diff --name-only 2>/dev/null | grep -q '\.rs$'; then
        FORMATTED=1
        print_info "Modified Rust files:"
        git diff --name-only | grep '\.rs$' | while read -r file; do
            echo -e "  ${YELLOW}~${NC} $file"
        done
    fi
else
    print_error "❌ Failed to format Rust code"
    exit 1
fi

# Format frontend code if it exists
if [ -d "frontend" ] && [ -f "frontend/package.json" ]; then
    print_info "Formatting frontend code..."

    # Check if node_modules exists
    if [ ! -d "frontend/node_modules" ]; then
        print_warning "node_modules not found. Installing dependencies..."
        (cd frontend && npm install)
    fi

    # Check if prettier is configured
    if [ -f "frontend/.prettierrc.json" ] || [ -f "frontend/.prettierrc" ] || [ -f "frontend/prettier.config.js" ]; then
        if (cd frontend && npm run format); then
            print_status "✅ Frontend code formatted successfully"

            # Check if any files were modified
            if git diff --name-only 2>/dev/null | grep -qE '\.(js|jsx|ts|tsx|json|css|md)$'; then
                FORMATTED=1
                print_info "Modified frontend files:"
                git diff --name-only | grep -E '\.(js|jsx|ts|tsx|json|css|md)$' | while read -r file; do
                    echo -e "  ${YELLOW}~${NC} $file"
                done
            fi
        else
            print_error "❌ Failed to format frontend code"
            exit 1
        fi
    else
        print_warning "No prettier configuration found, skipping frontend formatting"
    fi
fi

# Format TOML files
print_info "Formatting TOML files..."
if command -v taplo &> /dev/null; then
    if taplo fmt; then
        print_status "✅ TOML files formatted successfully"
    else
        print_warning "Failed to format TOML files"
    fi
else
    print_info "taplo not installed, skipping TOML formatting"
    print_info "Install with: cargo install taplo-cli"
fi

# Final result
echo ""
if [ $FORMATTED -eq 1 ]; then
    print_status "✅ Code formatting complete!"
    print_warning "Files have been modified. Please review the changes."
    print_info "Run 'git diff' to see the changes"
else
    print_status "✅ All code is already properly formatted!"
fi

exit 0
