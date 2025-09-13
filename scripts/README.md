# Scripts

This directory contains utility scripts for the m3u-proxy project.

## Available Scripts

### Development & Quality Scripts

#### `pre-commit.sh`
Comprehensive pre-commit checks that run automatically via git hooks.

```bash
./scripts/pre-commit.sh
```

**What it does:**
- Checks Rust formatting
- Runs Clippy linting
- Checks frontend formatting (if prettier is configured)
- Detects large files
- Scans for potential sensitive information
- Shows summary of files being committed

#### `format.sh`
Formats all code (Rust and frontend) automatically.

```bash
./scripts/format.sh
```

**What it does:**
- Formats all Rust code with `cargo fmt`
- Formats frontend code with prettier (if configured)
- Formats TOML files with taplo (if installed)
- Shows which files were modified

#### `check-format.sh`
Checks code formatting without making changes (CI-style).

```bash
./scripts/check-format.sh
```

**What it does:**
- Verifies Rust code formatting
- Verifies frontend code formatting
- Returns error if any formatting issues found

#### `lint.sh`
Runs comprehensive linting checks for code quality.

```bash
./scripts/lint.sh
```

**What it does:**
- Runs Clippy with strict settings
- Checks for `unwrap()` usage in production code
- Checks for `println!` in library code
- Finds TODO/FIXME comments
- Runs frontend ESLint (if configured)
- Checks for `console.log` statements
- Runs security audit (if cargo-audit installed)
- Checks for unused dependencies (if cargo-udeps installed)

#### `install-hooks.sh`
Installs git hooks for automated quality checks.

```bash
./scripts/install-hooks.sh
```

**What it does:**
- Installs pre-commit hook (runs formatting/linting checks)
- Installs commit-msg hook (validates conventional commit format)
- Installs pre-push hook (runs quick tests before pushing)
- Makes all scripts executable

**Commit Message Format:**
The commit-msg hook enforces conventional commits:
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Test additions/changes
- `build`: Build system changes
- `ci`: CI/CD changes
- `chore`: Maintenance tasks
- `revert`: Revert previous commit

Examples:
```
feat: add user authentication
fix(api): resolve connection timeout issue
docs: update installation instructions
```

### Release & Deployment Scripts

### `tag-release.sh`
Creates a new release tag with automatic semantic versioning.

```bash
# Patch bump: 0.2.3 -> 0.2.4
./scripts/tag-release.sh

# Minor bump: 0.2.3 -> 0.3.0  
./scripts/tag-release.sh minor

# Major bump: 0.2.3 -> 1.0.0
./scripts/tag-release.sh major
```

**Requirements:**
- Clean git working directory (no uncommitted changes)
- `just` command available (`cargo install just`)

**What it does:**
1. Validates git working directory is clean
2. Parses current version from Cargo.toml
3. Calculates new version based on bump type
4. Updates version in all project files (Cargo.toml, package.json, etc.)
5. Creates git commit for version bump
6. Creates and signs git tag
7. Shows next steps for pushing

### `build-container.sh`
Builds container images using detected container runtime (podman/docker/buildah/nerdctl).

```bash
./scripts/build-container.sh [VERSION]
```

### `push-container.sh`
Pushes container images to registry with proper version tagging.

```bash
./scripts/push-container.sh [--version VERSION] [REGISTRY]
```

## Usage via Justfile

All scripts can be called through the justfile for convenience:

```bash
# Development & Quality
just fmt                  # Format all code
just fmt-check           # Check formatting without changes
just lint                # Run linting checks
just pre-commit          # Run pre-commit checks
just install-hooks       # Install git hooks

# Tag release
just tag-release          # patch bump
just tag-release minor    # minor bump  
just tag-release major    # major bump

# Container operations
just build-container
just push-container [REGISTRY]
just build-container-versioned
just push-container-versioned [REGISTRY]
```

## Git Hooks

After running `./scripts/install-hooks.sh`, the following hooks are active:

1. **pre-commit**: Runs automatically before each commit
   - Formatting checks
   - Linting checks
   - Security scans
   - Can be bypassed with `git commit --no-verify`

2. **commit-msg**: Validates commit message format
   - Enforces conventional commit format
   - Can be bypassed with `git commit --no-verify`

3. **pre-push**: Runs quick checks before pushing
   - Format verification
   - Can be bypassed with `git push --no-verify`

## Quick Start for New Developers

```bash
# Install git hooks
just install-hooks

# Run all checks before committing
just pre-commit

# Format code if needed
just fmt

# Check what would be formatted
just fmt-check

# Run comprehensive linting
just lint
```