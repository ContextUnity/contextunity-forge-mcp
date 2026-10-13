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
invariants:
  - 'INV-DISTRIBUTION-PROFILES: Distribution must explicitly support Recommended Languages (default: Python, Rust, TS, JS, Vue, Proto, HTML, YAML, TOML), All Languages (all-languages: +Go, Java, C#, Kotlin, PHP, Ruby, C, C++), and Targeted Single Flavors (Python, Web, Rust).'
  - 'INV-ZERO-DEPENDENCY-INSTALL: The install.sh installer must run via standard POSIX sh using only curl/wget and tar, installing working binaries without requiring cargo, rustc, or C compilers for precompiled flavors.'
  - 'INV-SKILL-PARITY: Skills installed into global agent directories (~/.agents/skills/, ~/.claude/skills/) or repository-local (.agents/skills/) must match canonical contextunity-forge and acdd specifications.'
  - 'INV-ACDD-ONBOARDING: Adapter scaffolding must detect project language boundaries and generate valid, repo-tailored forge-mcp.yaml, AGENTS.md workflow guidance, and .mcp.json client configuration.'
  - 'INV-UPDATE-SEAMLESSNESS: The updater must cleanly detect existing installations, preserve user-tailored forge-mcp.yaml adapter configurations, and update binary and skills without data loss.'
related_plans: []
---

# Seamless installer, multi-flavor distribution, and ACDD onboarding

## Outcome and purpose

Deliver an end-to-end seamless installation, update, and onboarding experience for ContextUnity Forge MCP:
1. Establish fixed distribution profiles: Recommended Languages (default), All Languages (`all-languages`), and targeted single flavors (Python, Web, Rust).
2. Provide a zero-dependency POSIX `install.sh` (`curl -fsSL https://.../install.sh | bash`) supporting interactive flavor selection, custom source compilation, and automated self-updates.
3. Provide zero-config installation of agent skills (`contextunity-forge`, `acdd`) globally into agent environments (`~/.agents/skills/`, `~/.claude/skills/`) or repo-locally (`.agents/skills/`).
4. Provide intelligent scaffolding for repo-tailored `forge-mcp.yaml` adapters, `AGENTS.md` ACDD rules, and `.mcp.json` client registrations.
5. Establish a robust GitHub Actions release workflow for automated multi-platform binary compilation and release asset publishing.

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
    title: "Implement interactive menu and non-interactive CLI flags for recommended languages, all-languages, single flavors, or source cargo build"
    status: pending
  - subtask_ref: binary-placement-and-update-lifecycle
    title: "Implement atomic binary installation to ~/.local/bin/ and support seamless update check/upgrade command"
    status: pending
```

#### Acceptance

1. `install.sh` must execute in pure POSIX `sh` without bashisms, external language runtimes, or compilation tools when fetching precompiled releases.
2. Architecture and OS detection accurately handles Linux (`x86_64`, `aarch64`) and macOS (`x86_64`, `arm64`) with fallback prompt to build from source via Cargo.
3. Cryptographic integrity verified via SHA256 sums prior to unpack.
4. Binary atomically placed in `~/.local/bin/` with correct execute permissions, warning if `PATH` lacks directory.
5. Provide non-interactive mode via environment variables and flags (`--flavor`, `--prefix`, `--version`, `--yes`).

---

### task: agent-skills-and-acdd-onboarding

```yaml
task_ref: agent-skills-and-acdd-onboarding
target: "Automate global and local agent skill installation and project-tailored ACDD adapter scaffolding"
proof_policy: seam-test-first
contract_revision: 4
scope:
  - install.sh
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
```

#### Acceptance

1. Matrix workflow builds for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `aarch64-apple-darwin`.
2. Packages binaries for `recommended` (default), `all-languages`, and single-language flavors (`python`, `web`, `rust`).
3. Strips debug symbols, produces `.tar.gz` archives, and generates `SHA256SUMS`.
4. Publishes release assets and checksums to GitHub Releases upon semver tag push.
