#!/usr/bin/env bash
# Check code formatting without making changes
# Used by CI and pre-commit hooks

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
    echo -e "${GREEN}[FORMAT CHECK]${NC} $1"
}

print_error() {
    echo -e "${RED}[FORMAT CHECK ERROR]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[FORMAT CHECK WARNING]${NC} $1"
}

print_info() {
    echo -e "${BLUE}[FORMAT CHECK]${NC} $1"
}

# Change to project root
cd "$PROJECT_ROOT"

# Track if any checks fail
FAILED=0

# Check Rust formatting
print_info "Checking Rust code formatting..."
if cargo fmt --all -- --check; then
    print_status "✅ Rust formatting check passed"
else
    print_error "❌ Rust code needs formatting"
    print_warning "Run 'cargo fmt --all' or 'just fmt' to fix"
    FAILED=1
fi

# Check frontend formatting if it exists
if [ -d "frontend" ] && [ -f "frontend/package.json" ]; then
    print_info "Checking frontend code formatting..."

    # Check if node_modules exists
    if [ ! -d "frontend/node_modules" ]; then
        print_warning "node_modules not found. Installing dependencies..."
        (cd frontend && npm install)
    fi

    # Check if prettier is configured
    if [ -f "frontend/.prettierrc.json" ] || [ -f "frontend/.prettierrc" ] || [ -f "frontend/prettier.config.js" ]; then
        if (cd frontend && npm run format:check); then
            print_status "✅ Frontend formatting check passed"
        else
            print_error "❌ Frontend code needs formatting"
            print_warning "Run 'npm run format' in frontend/ or 'just fmt' to fix"
            FAILED=1
        fi
    else
        print_warning "No prettier configuration found, skipping frontend format check"
    fi
fi

# Final result
echo ""
if [ $FAILED -eq 0 ]; then
    print_status "✅ All formatting checks passed!"
    exit 0
else
    print_error "❌ Formatting checks failed!"
    print_error "Run 'just fmt' to fix formatting issues"
    exit 1
fi
