#!/usr/bin/env bash
#
# Tag Release Script
# 
# Creates a new release tag with automatic semantic versioning.
# Ensures clean git state, bumps version, creates commit, and tags.
#
# Usage: 
#   ./scripts/tag-release.sh [major|minor]
#   
# Examples:
#   ./scripts/tag-release.sh        # patch: 0.2.3 -> 0.2.4
#   ./scripts/tag-release.sh minor  # minor: 0.2.3 -> 0.3.0  
#   ./scripts/tag-release.sh major  # major: 0.2.3 -> 1.0.0

set -euo pipefail

# Change to project root directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

BUMP_TYPE="${1:-patch}"

# Validate bump type
if [[ ! "$BUMP_TYPE" =~ ^(patch|minor|major)$ ]]; then
    echo "Error: Invalid bump type '$BUMP_TYPE'"
    echo "Usage: $0 [major|minor]"
    echo "  No argument: patch bump (0.2.3 -> 0.2.4)"
    echo "  major:       major bump (0.2.3 -> 1.0.0)"
    echo "  minor:       minor bump (0.2.3 -> 0.3.0)"
    exit 1
fi

echo "🏷️  Tag Release - $BUMP_TYPE bump"
echo "Project: $(basename "$PROJECT_ROOT")"
echo ""

# Check if just is available (for version operations)
if ! command -v just >/dev/null 2>&1; then
    echo "Error: 'just' command not found"
    echo "Install with: cargo install just"
    exit 1
fi

# Check git status - must be clean
if ! git diff-index --quiet HEAD -- 2>/dev/null; then
    echo "Error: Working directory has unstaged changes"
    echo "Please commit or stash changes before tagging a release"
    echo ""
    git status --short
    exit 1
fi

# Check for staged changes
if ! git diff-index --quiet --cached HEAD -- 2>/dev/null; then
    echo "Error: Working directory has staged but uncommitted changes"
    echo "Please commit changes before tagging a release"
    echo ""
    git status --short
    exit 1
fi

# Get current version from Cargo.toml
CURRENT_VERSION=$(just get-current-version)
echo "📦 Current version: $CURRENT_VERSION"

# Parse current version
if [[ ! "$CURRENT_VERSION" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+) ]]; then
    echo "Error: Cannot parse current version '$CURRENT_VERSION'"
    echo "Expected format: X.Y.Z"
    exit 1
fi

MAJOR=${BASH_REMATCH[1]}
MINOR=${BASH_REMATCH[2]}  
PATCH=${BASH_REMATCH[3]}

# Calculate new version based on bump type
case "$BUMP_TYPE" in
    "major")
        NEW_MAJOR=$((MAJOR + 1))
        NEW_MINOR=0
        NEW_PATCH=0
        ;;
    "minor")
        NEW_MAJOR=$MAJOR
        NEW_MINOR=$((MINOR + 1))
        NEW_PATCH=0
        ;;
    "patch")
        NEW_MAJOR=$MAJOR
        NEW_MINOR=$MINOR
        NEW_PATCH=$((PATCH + 1))
        ;;
esac

NEW_VERSION="${NEW_MAJOR}.${NEW_MINOR}.${NEW_PATCH}"
TAG_NAME="v${NEW_VERSION}"

echo "🚀 Bumping version: $CURRENT_VERSION -> $NEW_VERSION ($BUMP_TYPE)"
echo "🏷️  Git tag will be: $TAG_NAME"

# Check if tag already exists
if git tag -l | grep -q "^$TAG_NAME$"; then
    echo "❌ Error: Git tag '$TAG_NAME' already exists"
    echo ""
    echo "Recent tags:"
    git tag -l | sort -V | tail -5
    exit 1
fi

echo ""
read -p "Continue with release? [y/N] " -n 1 -r
echo
if [[ ! $REPLY =~ ^[Yy]$ ]]; then
    echo "❌ Release cancelled"
    exit 1
fi

echo ""
echo "🔧 Updating version in project files..."
just set-version "$NEW_VERSION"

echo "📝 Creating git commit for version bump..."
git add .
git commit -m "chore: bump version to $NEW_VERSION"

echo "🏷️  Creating git tag: $TAG_NAME"
git tag -a "$TAG_NAME" -m "Release $NEW_VERSION"

echo ""
echo "✅ Release tagged successfully!"
echo "   Version: $CURRENT_VERSION -> $NEW_VERSION"
echo "   Git tag: $TAG_NAME"
echo "   Commit: $(git rev-parse --short HEAD)"
echo ""
echo "📤 Next steps:"
echo "   git push origin main"
echo "   git push origin $TAG_NAME"
echo "   # Or push both at once:"
echo "   git push origin main --tags"
echo ""
echo "🐳 To build and push container:"
echo "   just build-container-versioned"
echo "   just push-container-versioned [REGISTRY]"