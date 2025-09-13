#!/usr/bin/env bash
# Pre-commit hook script that runs quality checks before allowing commits
# This helps ensure code quality and prevents CI failures

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
    echo -e "${GREEN}[PRE-COMMIT]${NC} $1"
}

print_error() {
    echo -e "${RED}[PRE-COMMIT ERROR]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[PRE-COMMIT WARNING]${NC} $1"
}

print_info() {
    echo -e "${BLUE}[PRE-COMMIT]${NC} $1"
}

# Change to project root
cd "$PROJECT_ROOT"

# Check if we're in the right directory
if [ ! -f "Cargo.toml" ]; then
    print_error "Cargo.toml not found. Are you in the project root?"
    exit 1
fi

# Get the current branch name
BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "unknown")

print_status "Running pre-commit checks..."
print_info "Current branch: $BRANCH"

# Track if any checks fail
FAILED=0

# Function to run a check
run_check() {
    local name=$1
    local command=$2

    print_info "Running: $name..."
    if eval "$command"; then
        print_status "✅ $name passed"
    else
        print_error "❌ $name failed"
        FAILED=1
    fi
}

# Rust formatting check
print_info "Checking Rust formatting..."
if cargo fmt --all -- --check > /dev/null 2>&1; then
    print_status "✅ Rust formatting check passed"
else
    print_warning "Rust code needs formatting. Running formatter..."
    cargo fmt --all
    print_status "✅ Rust code formatted"
    print_warning "Files have been formatted. Please review and add them to your commit."
    git diff --name-only | grep '\.rs$' | while read -r file; do
        echo -e "  ${YELLOW}~${NC} $file"
    done
    FAILED=1
fi

# Rust linting (clippy)
print_info "Running Rust linter (clippy)..."
if cargo clippy --all-targets --all-features -- -D warnings 2>&1 | grep -q "error:"; then
    print_error "❌ Clippy found issues"
    cargo clippy --all-targets --all-features -- -D warnings
    FAILED=1
else
    print_status "✅ Clippy check passed"
fi

# Frontend checks (if frontend exists)
if [ -d "frontend" ] && [ -f "frontend/package.json" ]; then
    print_info "Checking frontend code..."

    # Check if node_modules exists
    if [ ! -d "frontend/node_modules" ]; then
        print_warning "node_modules not found. Installing dependencies..."
        (cd frontend && npm install)
    fi

    # Frontend formatting check
    if [ -f "frontend/.prettierrc.json" ]; then
        print_info "Checking frontend formatting..."
        if (cd frontend && npm run format:check > /dev/null 2>&1); then
            print_status "✅ Frontend formatting check passed"
        else
            print_warning "Frontend code needs formatting. Running formatter..."
            (cd frontend && npm run format)
            print_status "✅ Frontend code formatted"
            print_warning "Files have been formatted. Please review and add them to your commit."
            FAILED=1
        fi
    fi
fi

# Check for large files
print_info "Checking for large files..."
LARGE_FILES=$(git diff --cached --name-only | while read -r file; do
    if [ -f "$file" ]; then
        size=$(stat -f%z "$file" 2>/dev/null || stat -c%s "$file" 2>/dev/null || echo 0)
        if [ "$size" -gt 1048576 ]; then  # 1MB
            echo "$file ($(( size / 1024 / 1024 ))MB)"
        fi
    fi
done)

if [ -n "$LARGE_FILES" ]; then
    print_warning "Large files detected:"
    echo "$LARGE_FILES" | while read -r line; do
        echo -e "  ${YELLOW}!${NC} $line"
    done
    print_warning "Consider using Git LFS for large files"
fi

# Check for sensitive information
print_info "Checking for sensitive information..."
SENSITIVE_PATTERNS=(
    "password.*=.*['\"].*['\"]"
    "api[_-]?key.*=.*['\"].*['\"]"
    "secret.*=.*['\"].*['\"]"
    "token.*=.*['\"].*['\"]"
    "private[_-]?key"
)

for pattern in "${SENSITIVE_PATTERNS[@]}"; do
    if git diff --cached | grep -iE "$pattern" > /dev/null 2>&1; then
        print_warning "Possible sensitive information detected (pattern: $pattern)"
        print_warning "Please review your changes carefully"
    fi
done

# Show summary of what's being committed
print_info "Files being committed:"
git diff --cached --name-status | while read -r status file; do
    case $status in
        A) echo -e "  ${GREEN}+${NC} $file (added)" ;;
        M) echo -e "  ${YELLOW}~${NC} $file (modified)" ;;
        D) echo -e "  ${RED}-${NC} $file (deleted)" ;;
        R*) echo -e "  ${YELLOW}→${NC} $file (renamed)" ;;
        *) echo -e "  ${NC}?${NC} $file" ;;
    esac
done

# Final result
echo ""
if [ $FAILED -eq 0 ]; then
    print_status "✅ All pre-commit checks passed!"
    exit 0
else
    print_error "❌ Pre-commit checks failed!"
    print_error "Please fix the issues above before committing."
    print_warning "To bypass this check (not recommended), use: git commit --no-verify"
    exit 1
fi
