---
id: m-installer-distribution-and-acdd-onboarding
title: Seamless installer, multi-flavor distribution, and ACDD onboarding
doc_type: contract
status: planned
depends_on:
  - m-test-suite-architecture-and-domain-consolidation
owners:
  - src/engine/linker/
  - src/engine/ast/
  - src/cli/
  - .github/workflows/
  - install.sh
invariants:
  - 'INV-FEATURE-ISOLATION: Language profiles and tree-sitter bindings must be strictly decoupled; disabling lang-typescript must never break compilation of lang-python, lang-rust, or other profiles.'
  - 'INV-DISTRIBUTION-PROFILES: Distribution must explicitly support Recommended Languages (default: Python, Rust, TS, JS, Vue, Proto, HTML, YAML, TOML), All Languages (all-languages: +Go, Java, C#, Kotlin, PHP, Ruby, C, C++), and Targeted Single Flavors (Python, Web, Rust).'
  - 'INV-ZERO-DEPENDENCY-INSTALL: The install.sh installer must run via standard POSIX sh using only curl/wget and tar, installing working binaries without requiring cargo, rustc, or C compilers for precompiled flavors.'
  - 'INV-SKILL-PARITY: Skills installed into global agent directories (~/.agents/skills/, ~/.gemini/config/skills/, ~/.claude/skills/) or repository-local (.agents/skills/) must match canonical contextunity-forge and acdd specifications.'
  - 'INV-ACDD-ONBOARDING: Adapter scaffolding must detect project language boundaries and generate valid, repo-tailored forge-mcp.yaml, AGENTS.md workflow guidance, and .mcp.json client configuration.'
  - 'INV-UPDATE-SEAMLESSNESS: The updater must cleanly detect existing installations, preserve user-tailored forge-mcp.yaml adapter configurations, and update binary and skills without data loss.'
related_plans: []
---

# Seamless installer, multi-flavor distribution, and ACDD onboarding

## Outcome and purpose

Deliver an end-to-end seamless installation, update, and onboarding experience for ContextUnity Forge MCP:
1. Decouple compile-time language features so single-language flavors (`lang-python`, `lang-rust`, etc.) compile cleanly without cross-grammar linker dependencies.
2. Establish fixed distribution profiles: Recommended Languages (default), All Languages (`all-languages`), and targeted single flavors (Python, Web, Rust).
3. Provide a zero-dependency POSIX `install.sh` (`curl -fsSL https://.../install.sh | bash`) supporting interactive flavor selection, custom source compilation, and automated self-updates.
4. Provide zero-config installation of agent skills (`contextunity-forge`, `acdd`) globally or repo-locally.
5. Provide intelligent scaffolding for repo-tailored `forge-mcp.yaml` adapters, `AGENTS.md` ACDD rules, and `.mcp.json` client registrations.
6. Establish a robust GitHub Actions release workflow for automated multi-platform binary compilation and release asset publishing.

---

## Tasks in this milestone

### task: language-feature-decoupling-and-isolated-compilation

```yaml
task_ref: language-feature-decoupling-and-isolated-compilation
target: "Isolate cross-language linker and AST dependencies behind proper cfg-gates so single-language feature sets compile cleanly"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - src/engine/ast/mod.rs
  - src/engine/linker.rs
  - src/engine/linker/value_flow.rs
  - src/engine/languages/
  - Cargo.toml
status: planned
subtasks:
  - subtask_ref: cfg-gate-typescript-dom-bindings
    title: "Enclose typed_dom_property, typed_dom_receiver, and prototype_visible_at in src/engine/linker.rs and value_flow.rs under cfg(feature = 'lang-typescript') with fallback no-op stubs"
    status: pending
  - subtask_ref: cfg-gate-commonjs-callables
    title: "Enclose commonjs_default_callable in src/engine/ast/mod.rs under cfg(feature = 'lang-typescript') with fallback no-op stub"
    status: pending
  - subtask_ref: verify-isolated-language-compilation
    title: "Verify clean compilation of cargo check across isolated feature sets: lang-python, lang-rust, lang-typescript, and default"
    status: pending
```

### task: installer-script-and-distribution-automation

```yaml
task_ref: installer-script-and-distribution-automation
target: "Develop POSIX install.sh with platform detection, flavor selection, binary installation, and update capabilities"
proof_policy: seam-test-first
contract_revision: 2
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

### task: agent-skills-and-acdd-onboarding

```yaml
task_ref: agent-skills-and-acdd-onboarding
target: "Automate global and local agent skill installation and project-tailored ACDD adapter scaffolding"
proof_policy: seam-test-first
contract_revision: 2
scope:
  - install.sh
  - skills/
  - src/cli/guide.rs
  - docs/reference/configuration.md
status: planned
subtasks:
  - subtask_ref: skill-distribution-global-and-repo
    title: "Package and install canonical contextunity-forge and acdd skills into global (~/.agents/skills, ~/.gemini/config/skills, ~/.claude/skills) or local (.agents/skills)"
    status: pending
  - subtask_ref: project-aware-adapter-scaffolding
    title: "Implement repository language detection to scaffold pre-configured forge-mcp.yaml adapter with appropriate roots and ignore patterns"
    status: pending
  - subtask_ref: acdd-agents-rules-and-client-config
    title: "Scaffold AGENTS.md with canonical ACDD gates and generate .mcp.json client configuration"
    status: pending
```

### task: release-ci-cd-workflow

```yaml
task_ref: release-ci-cd-workflow
target: "Implement GitHub Actions release workflow for multi-platform, multi-flavor binary packaging"
proof_policy: seam-test-first
contract_revision: 2
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
