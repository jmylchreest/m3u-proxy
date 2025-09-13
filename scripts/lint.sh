#!/usr/bin/env bash
# Run linting checks for Rust and frontend code
# Checks for code quality issues, potential bugs, and style violations

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
    echo -e "${GREEN}[LINT]${NC} $1"
}

print_error() {
    echo -e "${RED}[LINT ERROR]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[LINT WARNING]${NC} $1"
}

print_info() {
    echo -e "${BLUE}[LINT]${NC} $1"
}

# Change to project root
cd "$PROJECT_ROOT"

# Track if any checks fail
FAILED=0

# Run Rust clippy
print_info "Running Rust linter (clippy)..."
if cargo clippy --all-targets --all-features -- -D warnings; then
    print_status "✅ Clippy check passed"
else
    print_error "❌ Clippy found issues"
    FAILED=1
fi

# Check for common Rust issues
print_info "Checking for common Rust issues..."

# Check for unwrap() calls in non-test code
print_info "Checking for unwrap() usage in production code..."
UNWRAP_COUNT=$(find crates -name "*.rs" -not -path "*/tests/*" -not -path "*/test/*" -not -name "*test*.rs" | xargs grep -h "\.unwrap()" 2>/dev/null | wc -l || echo 0)
if [ "$UNWRAP_COUNT" -gt 0 ]; then
    print_warning "Found $UNWRAP_COUNT unwrap() calls in production code"
    print_warning "Consider using expect() or proper error handling instead"
    find crates -name "*.rs" -not -path "*/tests/*" -not -path "*/test/*" -not -name "*test*.rs" | xargs grep -n "\.unwrap()" 2>/dev/null | head -5 || true
    if [ "$UNWRAP_COUNT" -gt 5 ]; then
        echo "  ... and $(( UNWRAP_COUNT - 5 )) more"
    fi
fi

# Check for println! in library code
print_info "Checking for println! in library code..."
PRINTLN_COUNT=$(find crates -name "*.rs" -not -path "*/examples/*" -not -path "*/tests/*" -not -name "main.rs" | xargs grep -h "println!" 2>/dev/null | wc -l || echo 0)
if [ "$PRINTLN_COUNT" -gt 0 ]; then
    print_warning "Found $PRINTLN_COUNT println! calls in library code"
    print_warning "Consider using proper logging (tracing, log, etc.) instead"
fi

# Check for TODO/FIXME comments
print_info "Checking for TODO/FIXME comments..."
TODO_COUNT=$(find crates -name "*.rs" | xargs grep -E "(TODO|FIXME)" 2>/dev/null | wc -l || echo 0)
if [ "$TODO_COUNT" -gt 0 ]; then
    print_warning "Found $TODO_COUNT TODO/FIXME comments"
    find crates -name "*.rs" | xargs grep -n -E "(TODO|FIXME)" 2>/dev/null | head -3 || true
    if [ "$TODO_COUNT" -gt 3 ]; then
        echo "  ... and $(( TODO_COUNT - 3 )) more"
    fi
fi

# Lint frontend code if it exists
if [ -d "frontend" ] && [ -f "frontend/package.json" ]; then
    print_info "Linting frontend code..."

    # Check if node_modules exists
    if [ ! -d "frontend/node_modules" ]; then
        print_warning "node_modules not found. Installing dependencies..."
        (cd frontend && npm install)
    fi

    # Run ESLint if available
    if grep -q '"lint"' frontend/package.json; then
        if (cd frontend && npm run lint); then
            print_status "✅ Frontend linting passed"
        else
            print_error "❌ Frontend linting failed"
            FAILED=1
        fi
    else
        print_warning "No lint script found in frontend/package.json"
    fi

    # Check for console.log in production code
    print_info "Checking for console.log in frontend code..."
    CONSOLE_COUNT=$(find frontend/src -name "*.ts" -o -name "*.tsx" -o -name "*.js" -o -name "*.jsx" | xargs grep -h "console\." 2>/dev/null | wc -l || echo 0)
    if [ "$CONSOLE_COUNT" -gt 0 ]; then
        print_warning "Found $CONSOLE_COUNT console statements in frontend code"
        print_warning "Consider using proper logging or removing debug statements"
    fi
fi

# Check for security issues with cargo-audit (if installed)
if command -v cargo-audit &> /dev/null; then
    print_info "Running security audit..."
    if cargo audit; then
        print_status "✅ Security audit passed"
    else
        print_warning "⚠️  Security audit found issues"
        # Don't fail on audit issues, just warn
    fi
else
    print_info "cargo-audit not installed, skipping security audit"
    print_info "Install with: cargo install cargo-audit"
fi

# Check for unused dependencies (if cargo-udeps is installed)
if command -v cargo-udeps &> /dev/null; then
    print_info "Checking for unused dependencies..."
    if cargo +nightly udeps --all-targets --quiet 2>/dev/null; then
        print_status "✅ No unused dependencies found"
    else
        print_warning "Found unused dependencies (or cargo-udeps failed)"
    fi
else
    print_info "cargo-udeps not installed, skipping unused dependency check"
    print_info "Install with: cargo install cargo-udeps"
    print_info "Note: requires nightly toolchain"
fi

# Final result
echo ""
if [ $FAILED -eq 0 ]; then
    print_status "✅ All linting checks passed!"
    exit 0
else
    print_error "❌ Linting checks failed!"
    print_error "Please fix the issues above"
    exit 1
fi
