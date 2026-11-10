---
id: m-installer-distribution-and-acdd-onboarding
title: Seamless installer, multi-flavor distribution, and ACDD onboarding
doc_type: contract
status: planned
depends_on:
  - m-test-suite-architecture-and-domain-consolidation
owners:
  - src/cli/
  - .github/workflows/
  - install.sh
  - skills/
  - build.rs
  - crates/forge-core/src/paths/
invariants:
  - 'INV-DISTRIBUTION-PROFILES: Distribution must explicitly support Recommended Languages (default: Python, Rust, TS, JS, Vue, Proto, HTML, YAML, TOML), All Languages (all-languages: experimental/under refinement; +Go, Java, C#, Kotlin, PHP, Ruby, C, C++), and Targeted Single Flavors (Python, Web, Rust).'
  - 'INV-ZERO-DEPENDENCY-INSTALL: The install.sh installer must run via standard POSIX sh using only curl/wget and tar, installing working binaries without requiring cargo, rustc, or C compilers for precompiled flavors.'
  - 'INV-SKILL-PARITY: Skills installed into global agent directories (~/.agents/skills/, ~/.claude/skills/) or repository-local (.agents/skills/) must match canonical contextunity-forge and acdd specifications.'
  - 'INV-ACDD-ONBOARDING: Adapter scaffolding must detect project language boundaries and generate valid, repo-tailored forge-mcp.yaml, AGENTS.md workflow guidance, and .mcp.json client configuration.'
  - 'INV-UPDATE-SEAMLESSNESS: The updater must cleanly detect existing installations, preserve user-tailored forge-mcp.yaml adapter configurations, update binary and skills without data loss, and execute contextunity-forge-mcp reload (sending SIGHUP) to seamlessly notify running MCP server processes.'
  - 'INV-POSIX-DARWIN-PARITY: Filesystem operations and compilation workflows on macOS must resolve /var and /tmp symlinks to canonical paths, preserve APFS case-casing, normalize Unicode paths to NFC, and configure C11/no-warnings flags for vendored Tree-sitter parsers.'
related_plans: []
---

# Seamless installer, multi-flavor distribution, and ACDD onboarding

## Outcome and purpose

Deliver an end-to-end seamless installation, update, and onboarding experience for ContextUnity Forge MCP:
1. Establish fixed distribution profiles: Recommended Languages (default, production-ready), Targeted Single Flavors (Python, Web, Rust), and All Languages (`all-languages` marked experimental / under active development).
2. Provide a zero-dependency POSIX `install.sh` executable via a single line (`curl -fsSL https://contextunity.org/install.sh | sh`) supporting auto-detection of OS (Linux, macOS) and architecture (x86_64, arm64), interactive flavor selection, automated `.tar.gz` extraction, and seamless self-updates.
3. Integrate skill distribution and project onboarding into `install.sh`: bundle automatic installation of companion skills into `~/.agents/skills/` (with symlinks to detected agent runtimes) and provide optional repo scaffolding (`forge-mcp.yaml`, `AGENTS.md`, `.mcp.json`).
4. Establish a GitHub Actions release workflow for tagged semver releases and automated `nightly` builds on push to `main` with stripped binaries, `.tar.gz` compression, and SHA256 verification sums.
5. Ensure robust POSIX compatibility for macOS (Darwin), resolving symlinks (/var, /tmp), APFS case-insensitivity, Unicode NFC normalization, and Apple Clang compiler standards.

---

## Tasks in this milestone

### task: installer-script-and-distribution-automation

```yaml
task_ref: installer-script-and-distribution-automation
target: "Develop POSIX install.sh with platform detection, flavor selection, binary installation, and update capabilities"
proof_policy: seam-test-first
contract_revision: 4
scope:
  - install.sh
  - src/cli/
  - docs/reference/mcp-setup.md
status: planned
subtasks:
  - subtask_ref: posix-platform-and-arch-detection
    title: "Implement POSIX platform detection for Linux (x86_64, aarch64) and macOS (x86_64, arm64) with SHA256 checksum verification"
    status: pending
  - subtask_ref: interactive-and-cli-flavor-selection
    title: "Implement interactive menu and non-interactive CLI flags for recommended languages, targeted flavors, experimental all-languages, or source cargo build"
    status: pending
  - subtask_ref: binary-placement-and-update-lifecycle
    title: "Implement atomic binary installation to ~/.local/bin/ with automatic tar.gz decompression, seamless update check/upgrade, and automatic SIGHUP reload of active servers"
    status: pending
  - subtask_ref: onboarding-and-skills-integration
    title: "Provide optional flags/prompts (--skills, --init) in install.sh to install companion skills globally and scaffold target repository adapter/guidance"
    status: pending
```

#### Acceptance

1. `install.sh` must execute in pure POSIX `sh` without bashisms, external language runtimes, or compilation tools when fetching precompiled releases (`curl -fsSL https://contextunity.org/install.sh | sh`).
2. Architecture and OS detection accurately handles Linux (`x86_64`, `aarch64`) and macOS (`x86_64`, `arm64`) with fallback prompt to build from source via Cargo.
3. Cryptographic integrity verified via SHA256 sums prior to unpack; `.tar.gz` distribution minimizes download bandwidth (~8 MB vs ~35 MB) while `install.sh` transparently decompresses and preserves execute permissions.
4. Binary atomically placed in `~/.local/bin/` with correct execute permissions, warning if `PATH` lacks directory.
5. Provide non-interactive mode via environment variables and flags (`--flavor`, `--prefix`, `--version`, `--yes`, `--skills`, `--init`).
6. Clearly mark `all-languages` in both CLI help and interactive selection as experimental / under active development.
7. Upon successful update of an existing binary, automatically trigger `contextunity-forge-mcp reload` (or SIGHUP) to ensure running MCP servers switch to the new executable without requiring client restarts.

---

### task: agent-skills-and-acdd-onboarding

```yaml
task_ref: agent-skills-and-acdd-onboarding
target: "Automate global and local agent skill installation and project-tailored ACDD adapter scaffolding"
proof_policy: seam-test-first
contract_revision: 4
depends_on:
  - installer-script-and-distribution-automation
scope:
  - skills/
  - src/cli/guide.rs
  - docs/reference/configuration.md
status: planned
subtasks:
  - subtask_ref: skill-distribution-global-and-repo
    title: "Package and install canonical contextunity-forge and acdd skills into global (~/.agents/skills, ~/.claude/skills) or local (.agents/skills)"
    status: pending
  - subtask_ref: project-aware-adapter-scaffolding
    title: "Implement repository language detection to scaffold pre-configured forge-mcp.yaml adapter with appropriate roots and ignore patterns"
    status: pending
  - subtask_ref: acdd-agents-rules-and-client-config
    title: "Scaffold AGENTS.md with canonical ACDD gates and generate .mcp.json client configuration"
    status: pending
```

#### Acceptance

1. Package canonical `skills/contextunity-forge/` and `acdd` skills and provide automated copy/sync into detected agent configuration directories:
   - `~/.agents/skills/`
   - `~/.claude/skills/`
   - Repository-local `.agents/skills/`
2. Ensure installed skill content matches canonical repository specifications and preserves YAML frontmatter integrity.
3. Automatically detect primary repository languages and generate idiomatic `forge-mcp.yaml` adapter configuration with sensible include roots and standard ignore patterns.
4. Scaffold `AGENTS.md` containing ACDD lifecycle routing and create `.mcp.json` / Claude desktop config entries.

---

### task: release-ci-cd-workflow

```yaml
task_ref: release-ci-cd-workflow
target: "Implement GitHub Actions release workflow for multi-platform, multi-flavor binary packaging"
proof_policy: seam-test-first
contract_revision: 4
scope:
  - .github/workflows/release.yml
status: planned
subtasks:
  - subtask_ref: matrix-multi-target-compilation
    title: "Configure GitHub Actions build matrix for Linux x86_64/aarch64 and macOS x86_64/arm64 across recommended, all-languages, and targeted flavors"
    status: pending
  - subtask_ref: release-packaging-and-checksums
    title: "Implement artifact stripping, tar.gz compression, and SHA256 generation"
    status: pending
  - subtask_ref: automated-github-releases
    title: "Publish release archives and checksums to GitHub Releases on tag push"
    status: pending
  - subtask_ref: automated-nightly-builds
    title: "Configure automated rolling nightly builds and release artifacts on push to main"
    status: pending
```

#### Acceptance

1. Matrix workflow builds for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `aarch64-apple-darwin`.
2. Packages binaries for `recommended` (default), `all-languages` (experimental), and single-language flavors (`python`, `web`, `rust`).
3. Strips debug symbols, produces `.tar.gz` archives, and generates `SHA256SUMS`.
4. Publishes release assets and checksums to GitHub Releases upon semver tag push (`v*`).
5. Generates and updates automated rolling `nightly` release builds on every commit push to `main`.

---

### task: macos-posix-runtime-and-filesystem-compatibility

```yaml
task_ref: macos-posix-runtime-and-filesystem-compatibility
target: "Ensure seamless macOS POSIX compatibility for canonical paths, APFS case-insensitivity, Unicode NFC, and Darwin C builds"
proof_policy: seam-test-first
contract_revision: 1
scope:
  - build.rs
  - src/core/paths.rs
  - crates/forge-core/src/
status: planned
subtasks:
  - subtask_ref: canonical-workspace-root-and-symlinks
    title: "Implement CanonicalWorkspaceRoot to resolve Darwin /var -> /private/var and /tmp -> /private/tmp symlinks without prefix breakage"
    status: pending
  - subtask_ref: apfs-case-preservation
    title: "Implement normalize_repo_relative_path to preserve physical disk casing via canonicalize/F_GETPATH on APFS"
    status: pending
  - subtask_ref: unicode-nfc-normalization
    title: "Enforce Unicode NFC path normalization across SQLite path indexing and milestone contracts"
    status: pending
  - subtask_ref: darwin-c-build-flags
    title: "Configure build.rs cc::Build with C11 standard and disabled warnings for Apple Clang compatibility"
    status: pending
```

#### Acceptance

1. `CanonicalWorkspaceRoot` resolves Darwin `/var` and `/tmp` symlinks to `/private/var` and `/private/tmp` at workspace initialization, preventing prefix stripping errors in `tempfile::TempDir` and worktree child paths.
2. Existing files preserve true physical casing on APFS via `canonicalize()` / `fcntl(F_GETPATH)`, preventing case mismatch desync between SQLite locks and disk.
3. All repository-relative path comparisons and stored SQLite keys enforce Unicode NFC normalization.
4. `build.rs` compiles vendored C Tree-sitter grammars cleanly with Apple Clang on ARM64 macOS without implicit declaration errors.
