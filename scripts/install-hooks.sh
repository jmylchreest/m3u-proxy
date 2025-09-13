#!/usr/bin/env bash
# Install git hooks for the project
# This script sets up pre-commit and other git hooks

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
    echo -e "${GREEN}[INSTALL]${NC} $1"
}

print_error() {
    echo -e "${RED}[INSTALL ERROR]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[INSTALL WARNING]${NC} $1"
}

print_info() {
    echo -e "${BLUE}[INSTALL]${NC} $1"
}

# Change to project root
cd "$PROJECT_ROOT"

# Check if we're in a git repository
if [ ! -d ".git" ]; then
    print_error "Not in a git repository. Please run this from the project root."
    exit 1
fi

print_info "Installing git hooks..."

# Create hooks directory if it doesn't exist
HOOKS_DIR=".git/hooks"
if [ ! -d "$HOOKS_DIR" ]; then
    mkdir -p "$HOOKS_DIR"
    print_info "Created hooks directory"
fi

# Install pre-commit hook
print_info "Installing pre-commit hook..."
cat > "$HOOKS_DIR/pre-commit" << 'EOF'
#!/usr/bin/env bash
# Git pre-commit hook that runs project checks
# This hook is installed by scripts/install-hooks.sh

# Find the project root (where .git is located)
PROJECT_ROOT="$(git rev-parse --show-toplevel)"

# Check if the pre-commit script exists
if [ -f "$PROJECT_ROOT/scripts/pre-commit.sh" ]; then
    # Run the pre-commit script
    exec "$PROJECT_ROOT/scripts/pre-commit.sh"
else
    echo "Warning: pre-commit.sh script not found at $PROJECT_ROOT/scripts/pre-commit.sh"
    echo "Skipping pre-commit checks..."
    exit 0
fi
EOF

chmod +x "$HOOKS_DIR/pre-commit"
print_status "✅ Pre-commit hook installed"

# Install commit-msg hook for conventional commits (optional)
print_info "Installing commit-msg hook..."
cat > "$HOOKS_DIR/commit-msg" << 'EOF'
#!/usr/bin/env bash
# Git commit-msg hook that validates commit messages
# This hook is installed by scripts/install-hooks.sh

# Read the commit message
COMMIT_MSG_FILE=$1
COMMIT_MSG=$(cat "$COMMIT_MSG_FILE")

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if commit message follows conventional commits format
# Format: type(scope): description
# Examples:
#   feat: add new feature
#   fix(api): resolve connection issue
#   docs: update README

# Valid types
VALID_TYPES="feat|fix|docs|style|refactor|perf|test|build|ci|chore|revert"

# Check commit message format (basic check)
if echo "$COMMIT_MSG" | grep -qE "^($VALID_TYPES)(\(.+\))?: .{1,}$"; then
    echo -e "${GREEN}[COMMIT]${NC} ✅ Commit message format is valid"
    exit 0
fi

# Check if it's a merge commit
if echo "$COMMIT_MSG" | grep -qE "^Merge "; then
    exit 0
fi

# Check if it's a revert commit
if echo "$COMMIT_MSG" | grep -qE "^Revert "; then
    exit 0
fi

# If not valid, show error and examples
echo -e "${RED}[COMMIT ERROR]${NC} Invalid commit message format!"
echo -e "${YELLOW}[COMMIT]${NC} Expected format: type(scope): description"
echo -e "${YELLOW}[COMMIT]${NC} Valid types: $VALID_TYPES"
echo ""
echo "Examples:"
echo "  feat: add user authentication"
echo "  fix(api): resolve connection timeout issue"
echo "  docs: update installation instructions"
echo "  refactor(cli): simplify argument parsing"
echo ""
echo "Your message: $COMMIT_MSG"
echo ""
echo "To bypass this check, use: git commit --no-verify"

exit 1
EOF

chmod +x "$HOOKS_DIR/commit-msg"
print_status "✅ Commit-msg hook installed"

# Install pre-push hook (optional - runs tests before push)
print_info "Installing pre-push hook..."
cat > "$HOOKS_DIR/pre-push" << 'EOF'
#!/usr/bin/env bash
# Git pre-push hook that runs tests before pushing
# This hook is installed by scripts/install-hooks.sh

# Find the project root
PROJECT_ROOT="$(git rev-parse --show-toplevel)"

# Colors for output
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo -e "${GREEN}[PRE-PUSH]${NC} Running quick tests before push..."

# Run quick tests (not full test suite to keep it fast)
if [ -f "$PROJECT_ROOT/justfile" ] && command -v just &> /dev/null; then
    # If we have just, use it for quick checks
    (cd "$PROJECT_ROOT" && just fmt-check)
else
    # Otherwise just run cargo check
    (cd "$PROJECT_ROOT" && cargo check)
fi

if [ $? -eq 0 ]; then
    echo -e "${GREEN}[PRE-PUSH]${NC} ✅ Pre-push checks passed"
    exit 0
else
    echo -e "${YELLOW}[PRE-PUSH]${NC} ⚠️  Pre-push checks failed"
    echo -e "${YELLOW}[PRE-PUSH]${NC} Fix the issues or use 'git push --no-verify' to bypass"
    exit 1
fi
EOF

chmod +x "$HOOKS_DIR/pre-push"
print_status "✅ Pre-push hook installed"

# Make all scripts executable
print_info "Making all scripts executable..."
chmod +x "$SCRIPT_DIR"/*.sh
print_status "✅ All scripts are executable"

# Show summary
echo ""
print_status "🎉 Git hooks installation complete!"
echo ""
echo "Installed hooks:"
echo "  • pre-commit  - Runs formatting and linting checks"
echo "  • commit-msg  - Validates commit message format"
echo "  • pre-push    - Runs quick tests before pushing"
echo ""
echo "To skip hooks temporarily, use --no-verify flag:"
echo "  git commit --no-verify"
echo "  git push --no-verify"
echo ""
echo "To uninstall hooks, run:"
echo "  rm .git/hooks/pre-commit .git/hooks/commit-msg .git/hooks/pre-push"
