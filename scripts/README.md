# Scripts

This directory contains utility scripts for the m3u-proxy project.

## Available Scripts

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