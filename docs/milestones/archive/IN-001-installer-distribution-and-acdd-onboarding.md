---
id: m-installer-distribution-and-acdd-onboarding
title: Seamless installer, multi-flavor distribution, and ACDD onboarding
doc_type: contract
status: completed
depends_on:
- m-test-suite-architecture-and-domain-consolidation
owners:
- install.sh
- .github/workflows/
- crates/forge-index/build.rs
- skills/contextunity-forge/
- docs/reference/
polish_scope:
- INSTALL.md
- README.md
- docs/reference/cli.md
invariants:
- 'INV-ZERO-DEPENDENCY-INSTALL: The install.sh installer must run via standard POSIX sh using only curl/wget and tar, installing working binaries and invoking skills/install.sh without requiring cargo, rustc, or C compilers.'
- 'INV-NO-SKILLS-IN-BINARY: Skills are pure companion assets, NEVER embedded into the Rust binary via build.rs codegen, include_bytes!, or internal Rust CLI installers. install.sh distributes and places companion skills into ~/.agents/skills/ directly from release archives or GitHub.'
- 'INV-UPDATE-SEAMLESSNESS: The installer/updater must cleanly detect existing installations, preserve user-tailored configurations, update binary and skills without data loss, and execute contextunity-forge-mcp reload (sending SIGHUP) to seamlessly notify running MCP server processes.'
- 'INV-AGENTIC-ONBOARDING: Repository onboarding and scaffolding is driven by agent skills, leveraging LLM context to generate tailored forge-mcp.yaml (with linked_workspaces), ACDD profiles, AGENTS.md, documentation structures, and initial milestones without bloating the Rust binary.'
related_plans: []
started_at: 2026-11-10T15:51:41+00:00
repository: forge-mcp
project: forge-mcp
handoff:
  completed_at: 2026-11-11T04:06:22.420983290+00:00
  duration: 12h 14m
  commit: fc378e3209f218514cdd37c6b6d29b6c993e821b
  target_base: 716c4fa15fd054917291d07ce5a22a98623ef00d
  integration_evidence:
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-installer-distribution-and-acdd-onboarding
      authority_digest: 9794ddf242cc901de0ec185dee02fde79b3654a183d31d9757bc18df3f362a86
      implementation_digest: 1138ad12843e4d2956f7214b0c4a7a51f3c34c995946fd3aab95e82ec9eb9cf8
      target_base_digest: daf6a4c93616f5082777a31b3f98e3db373bb7500ccc50479ddfa85fa8004ff8
      builders:
      - codex-in003-builder
      - installer-builder
      - release-builder
    target_base: 716c4fa15fd054917291d07ce5a22a98623ef00d
    review:
      worker_id: codex-sol-reviewer
      decision: pass
      summary: 'All milestone invariants verified: install.sh POSIX compliance and zero-dependency packaging, CI/CD multi-flavor distribution, agentic onboarding skill, clean process environment isolation, and comprehensive test suite passing with zero failures.'
      scope_findings: []
      contract_findings: []
    verification:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 1208
      tests_failed: 0
      project_command: check_full_repo
  proof_shas:
  - 0b2a9a053ea5f1af29132cef0f33f1ad04507186
  - 1f473231c2b729dd5ae538b3573b1179f4ca4338
  - 221146c046c63ad6188f8920842ddc9f810fd010
  - 33566b90117ca08a3b5f22edd9d524f3a4be6a48
  - 527ad39ffb4fee03b7ec75c5710f69ede6ceb9f8
  - 5ea71c478b5a51a00dcc813ea53554f4bdaba69f
  - 65d4cc00b7cebcf91256354578134b0d47a0b2a8
  - 66c796f32b2ce67bc229b1eeb36d200cebadbfee
  - 677b20365738115aa4dea901e614cf172fbd6fad
  - 716c4fa15fd054917291d07ce5a22a98623ef00d
  - 7684bfad0c6b326a3af1649c1a9b0e121553db94
  - 84b069c988658995e54cd3a6996fd7e5b03cfa85
  - 938e30b608b4a409eaf7c9f2033665b79815f110
  - afe46cbca71068f8a250ca8566bfe0e5b18b6973
  - e4d87741e3c26f9b0e96d28ca02b679d1cd275fb
  - e98100e5c0ca3c3adcb171157cfd064b8eec647e
  - ea31524cc86a97f658f69306a21d10c841854965
  - fc378e3209f218514cdd37c6b6d29b6c993e821b
  passport:
    version: 1
    identity: forge-mcp/forge-mcp/m-installer-distribution-and-acdd-onboarding
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: m-installer-distribution-and-acdd-onboarding
      authority_digest: 9794ddf242cc901de0ec185dee02fde79b3654a183d31d9757bc18df3f362a86
      implementation_digest: 1138ad12843e4d2956f7214b0c4a7a51f3c34c995946fd3aab95e82ec9eb9cf8
      target_base_digest: daf6a4c93616f5082777a31b3f98e3db373bb7500ccc50479ddfa85fa8004ff8
      builders:
      - codex-in003-builder
      - installer-builder
      - release-builder
    commit: fc378e3209f218514cdd37c6b6d29b6c993e821b
    target_base: 716c4fa15fd054917291d07ce5a22a98623ef00d
    active_authority_digest: 8dbb976090f4fff5c5ef3d7122088ed9888d9a06692fede06459489fa77558ee
    started_at: 2026-11-10T15:51:41+00:00
    completed_at: 2026-11-11T04:06:22.420983290+00:00
    duration_seconds: 44081
    tasks_count: 3
    tasks:
    - task_id: forge-mcp/m-installer-distribution-and-acdd-onboarding:acdd-onboarding-agent-skill
      status: completed
      first_claimed_at: 1794325901
      completed_at: 1794331455
      execution_wall_seconds: 5554
      review_cycles: 2
      rejected_reviews: 1
    - task_id: forge-mcp/m-installer-distribution-and-acdd-onboarding:ci-cd-and-release-packaging
      status: completed
      first_claimed_at: 1794333455
      completed_at: 1794335233
      execution_wall_seconds: 1778
      review_cycles: 1
      rejected_reviews: 0
    - task_id: forge-mcp/m-installer-distribution-and-acdd-onboarding:installer-script-and-client-setup
      status: completed
      first_claimed_at: 1794333436
      completed_at: 1794338934
      execution_wall_seconds: 5498
      review_cycles: 4
      rejected_reviews: 3
    diff:
      docs:
        files: 3
        added: 340
        removed: 131
      other:
        files: 9
        added: 603
        removed: 137
      src:
        files: 2
        added: 292
        removed: 21
      tests:
        files: 4
        added: 1650
        removed: 0
    ast_symbol_deltas:
      added: 128
      removed: 0
      modified: 2
    test_suite_results:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 1208
      tests_failed: 0
      project_command: check_full_repo
    automatic_project_verification:
      selection: check_full_repo
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      stdout_digest: df43559b60ac6306ea1b833c85245b7af058cd3f6c51ea6dfb01c4c383323dbb
      reused_verification: true
      implementation_digest: 1138ad12843e4d2956f7214b0c4a7a51f3c34c995946fd3aab95e82ec9eb9cf8
    linter:
      status: unobserved
    topology:
      upstream:
      - forge-mcp/forge-mcp/m-test-suite-architecture-and-domain-consolidation
      downstream:
      - forge-mcp/forge-mcp/m-source-modularity-and-god-file-decomposition
      unresolved_upstream: []
    verified_handoff_proof: 6aa59fbd22a4289b7f44592a0f41c0335c21a3f4dd0ffd377e813fd87bc09aee
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 1208
    tests_failed: 0
---

# Seamless installer, multi-flavor distribution, and ACDD onboarding

## Outcome and purpose

Deliver an end-to-end installation, packaging, and agentic project onboarding experience for ContextUnity Forge MCP:
1. Establish GitHub Actions CI and Release pipelines for multi-platform compilation (Linux x86_64/arm64, macOS x86_64/arm64), tar.gz packaging with companion skills, and SHA256 checksum verification.
2. Provide a zero-dependency POSIX `install.sh` executable via a single line (`curl -fsSL https://.../install.sh | sh`), auto-detecting platform, placing the binary in `~/.local/bin/`, executing `skills/install.sh` to populate agent skills, and configuring project-level or global `.mcp.json`.
3. Provide an interactive Agentic Onboarding Skill (`skills/contextunity-forge/references/onboarding.md` and `SKILL.md`) enabling AI agents to inspect any repository, configure tailored `forge-mcp.yaml` (with `linked_workspaces`), synthesize `.forge/acdd/profile.yaml`, author `AGENTS.md`, bootstrap documentation layout, and propose initial milestones and plans.

---

## Architectural Boundaries ("Як не треба робити" — Anti-Patterns Prohibited)

1. **NO SKILLS IN BINARY**: Never embed skills into the Rust binary using `build.rs`, `include_bytes!`, or internal Rust CLI installation modules. Skills are Markdown documents distributed via the release archive `.tar.gz` and placed on disk by `skills/install.sh`.
2. **NO RUST IN INSTALLER**: `install.sh` is pure, self-contained POSIX `sh`. It must not invoke `cargo`, `rustc`, C compilers, or require a Rust development environment to install precompiled releases.
3. **NO RUNAWAY SCOPE MUTATIONS**: Do not touch `crates/forge-core/src/fs.rs`, `git_snapshot.rs`, root `Cargo.toml`, or unrelated subsystems. No ad-hoc filesystem gymnastics or Unicode normalization hacks in Rust.
4. **REUSE EXISTING ASSETS**: Use existing `skills/install.sh` for agent skill distribution and linking; do not duplicate skill syncing logic.
5. **NO SCAFFOLDING CODE BLOAT IN BINARY**: Do not implement heuristic language detectors, project scanners, or template renderers in `src/cli/` or Rust code. All onboarding intelligence belongs to the agent onboarding skill.

---

## Tasks in this milestone

### task: ci-cd-and-release-packaging

```yaml
task_ref: ci-cd-and-release-packaging
target: Implement GitHub Actions CI and multi-platform release packaging workflow with companion skills
proof_policy: seam-test-first
contract_revision: 2
scope:
- .github/workflows/
- crates/forge-index/build.rs
- tests/release_packaging.rs
status: completed
subtasks:
- subtask_ref: continuous-integration-workflow
  title: Configure .github/workflows/ci.yml with check, clippy, and test across Linux and macOS runners
  status: completed
  evidence: 'Implemented .github/workflows/ci.yml for PR/main on ubuntu-24.04 and macos-15: workspace cargo check, strict workspace Clippy all-targets/all-features, workspace tests. Architecture guard ci_checks_pull_requests_and_main_on_linux_and_macos passed. Accepted build snapshot 7684bfad0c6b326a3af1649c1a9b0e121553db94; claim-bound strict Clippy + release_packaging 4 passed/0 failed. Remote native CI execution remains a workflow-runtime boundary.'
- subtask_ref: matrix-multi-target-compilation
  title: Configure .github/workflows/release.yml build matrix for Linux (x86_64, aarch64) and macOS (x86_64, arm64)
  status: completed
  evidence: 'Implemented release.yml native mapping: ubuntu-24.04/x86_64-unknown-linux-gnu; ubuntu-24.04-arm/aarch64-unknown-linux-gnu; macos-15-intel/x86_64-apple-darwin; macos-15/aarch64-apple-darwin. cargo build --release --locked --target TARGET. Tag v* and main/nightly guard passed in release_packaging; claim-bound strict Clippy + suite 4 passed/0 failed, snapshot 7684bfad0c6b326a3af1649c1a9b0e121553db94. Four-platform compilation delegated to actual native workflow runners.'
- subtask_ref: release-packaging-and-checksums
  title: Bundle stripped binary and companion skills directory into tar.gz archives and generate SHA256SUMS
  status: completed
  evidence: 'Actual YAML package shell executed successfully in release_package_step_produces_installable_archive_and_checksum: archive SHA256 matched, extracted real CLI --version ran, all companion skills bytes preserved including skills/install.sh. Existing Cargo release strip=true is canonical stripping owner. Publisher actual fan-in shell additionally executed against four separate target artifact fixtures: four unique checksum records retained and all four SHA256 checks passed. Claim-bound strict Clippy + release_packaging 4 passed/0 failed; snapshot 7684bfad0c6b326a3af1649c1a9b0e121553db94. No remote publication performed.'
- subtask_ref: darwin-c-build-flags
  title: Configure crates/forge-index/build.rs with C11 standard and disabled warnings for Apple Clang compatibility
  status: completed
  evidence: crates/forge-index/build.rs existing cc::Build configuration now includes .std("c11").warnings(false), preserving existing proto include/file/compile owner. proto_c_compilation_selects_c11_without_compiler_warnings guard passed; local forge-index compiled and claim-bound strict Clippy + release_packaging suite passed 4/0. Accepted snapshot 7684bfad0c6b326a3af1649c1a9b0e121553db94. Actual Apple Clang compilation is delegated to native macOS CI/release jobs.
receipt:
  delivery_gate: deliver
  candidate_gate: build
  commit: d988be7fda339ee0db6e4da666733793792e4c3e
  contract_revision: 2
  passed_at: 2026-11-10T18:27:13.975127542+00:00
  evidence:
    command_proof:
      command: rust-check-and-test
      exit_code: 0
      log: |
        [rust-check-and-test::tests/release_packaging.rs]
        SCOPE_VIOLATION_WARNING: docs/milestones/IN-001-installer-distribution-and-acdd-onboarding.md
        PASS rust-check-and-test::tests/release_packaging.rs: exit=0 passed=4 failed=0 (18397 ms)
      tests_passed: 4
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Four admitted blobs exactly match candidate 7684bfa: ci 057d634c, release 39ca2ed6, forge-index/build.rs 1df80ea2, tests 52e66379. Only these task files changed; coordinator manifest admission matches BB #8. Protected root Cargo.toml/build.rs/.forge untouched; git diff --check clean. Reviewer made no source edits.'
        claims:
          applicable: true
          evidence: Acceptance 1-5 traced to real YAML PR/main CI check/strict all-features workspace Clippy/tests, v*/main release triggers, four native target matrix, release strip=true, companion archive and four-record SHA256SUMS, proto C11/warnings(false). cargo clippy --test release_packaging --all-features -- -D warnings exit0; cargo test --test release_packaging 4 passed/0 failed/0 ignored. Actual package shell archives real CLI, extracts, runs --version and preserves all companion bytes. 14 isolated behavioral cases validate fan-in and negative/publisher paths; report /tmp/in001 release review n7_bukes/report.json. Hosted native compilation/publication remain explicit unexecuted boundary.
        concurrency:
          applicable: true
          evidence: Release workflow serializes each tag and rolling nightly, leaves running job uncanceled, and checks current main SHA before publishing. Actual publisher shell with local gh fixture confirms first/rerun release behavior, superseded nightly no mutation, explicit nightly ref PATCH, and API/upload errors fail. needs:build plus separate artifact directories and count/checksum validation precede publish; four distinct checksums survive fan-in. No ACDD storage/receipt concurrency code changed.
        project_isolation:
          applicable: true
          evidence: 'Packaging and fan-in run in fresh runner workspaces with quoted TARGET/staging/archive paths and cleanup trap; temporary probe root deliberately contains spaces. Public asset naming/layout matches BB #7 and installer consumer. Tests/probes use isolated temp directories, preserve user HOME and sibling repositories, and replace gh with local fixture; no remote write or publication. No workspace/profile/path resolution runtime changed.'
        administration:
          applicable: true
          evidence: CI/build use contents:read; only post-build publisher receives contents:write and GH_TOKEN. Environment indirection/quoting prevents ref interpolation into shell. Native runner labels verified from official requested GitHub docs. Root release optimization/strip profile preserved, no source runtime/performance algorithm added. Whole-workspace duplicate scan has 19 preexisting groups; no introduced duplicate algorithm. Candidate binding stable, coordinator admission intact, no manual Git commit, receipt edit, benchmark or release performed.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ZERO-DEPENDENCY-INSTALL: The install.sh installer must run via standard POSIX sh using only curl/wget and tar, installing working binaries and invoking skills/install.sh without requiring cargo, rustc, or C compilers.'
    - 'INV-NO-SKILLS-IN-BINARY: Skills are pure companion assets, NEVER embedded into the Rust binary via build.rs codegen, include_bytes!, or internal Rust CLI installers. install.sh distributes and places companion skills into ~/.agents/skills/ directly from release archives or GitHub.'
    - 'INV-UPDATE-SEAMLESSNESS: The installer/updater must cleanly detect existing installations, preserve user-tailored configurations, update binary and skills without data loss, and execute contextunity-forge-mcp reload (sending SIGHUP) to seamlessly notify running MCP server processes.'
    - 'INV-AGENTIC-ONBOARDING: Repository onboarding and scaffolding is driven by agent skills, leveraging LLM context to generate tailored forge-mcp.yaml (with linked_workspaces), ACDD profiles, AGENTS.md, documentation structures, and initial milestones without bloating the Rust binary.'
    architectural_notes:
    - 'decisions: {"decision":"independent-release-review-pass","candidate":"7684bfad0c6b326a3af1649c1a9b0e121553db94","claim_revision":20,"contract_revision":2,"binding":"Four admitted worktree blob IDs exactly match scope-only candidate. Coordinator-owned manifest admission matches milestone decision #8; no reviewer source edits.","authority":["Acceptance 1-5","archive interface decision #7","architecture/proof map #9/#13","checksum fan-in note #11"],"observations":{"packaging":"Production YAML package shell -> real test CLI archive -> SHA256 readback -> tar extraction -> executable --version -> all companion bytes; 4 focused tests pass and scoped all-features strict Clippy exits 0.","negative_and_publisher_probe":"14 isolated cases execute extracted production YAML shell. Missing binary/skills/companion installer, missing target/checksum, archive corruption fail. Complete four-record fan-in succeeds. Local gh fixture validates tag create/rerun, nightly create/ref PATCH/rerun, superseded skip, API query failure and upload failure propagation.","report":"/tmp/in001 release review n7_bukes/report.json","runner_mapping":"2026-11-10 official requested GitHub runner docs confirm ubuntu-24.04/x64, ubuntu-24.04-arm/ARM64, macos-15-intel/Intel and macos-15/ARM64.","sources":["https://docs.github.com/en/actions/how-tos/write-workflows/choose-where-workflows-run/choose-the-runner-for-a-job","https://cli.github.com/manual/gh_release_create","https://cli.github.com/manual/gh_release_edit","https://docs.github.com/en/actions/how-tos/write-workflows/choose-when-workflows-run/control-workflow-concurrency"],"concurrency":"Per-tag/nightly workflow-level serialization plus main SHA check prevents queued stale nightly replacing a newer accepted run; retry uses create-if-missing and upload --clobber. Explicit PATCH updates existing nightly Git ref.","privilege":"PR CI/build contents:read. Only post-fan-in publisher contents:write; token scoped to publisher. Matrix/ref/repository values passed through quoted environment variables; no local remote publication.","duplicates":"Whole-workspace clone scan finds 19 pre-existing groups, none introduced by this candidate. Changed test owner has no clone groups. Existing build-script clones remain outside this bounded change; no parallel algorithm added.","performance":"No runtime indexing/storage path changed. Existing release strip=true, lto=thin and codegen-units=1 preserved."},"proof_boundaries":["Native four-platform compilation and actual hosted GitHub Actions execution remain workflow-runtime verification, not proven by local host packaging.","Publication commands are exercised against a local gh fixture; no release, tag, push, credential access or benchmark performed.","CI triggers/matrix/C11 flag checks are architecture guards, not evidence of hosted workflow success."],"findings":[]}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `.github/workflows/ci.yml` executes on pull requests and pushes to `main`, validating `cargo clippy --workspace --all-targets --all-features -- -D warnings` and `cargo test`.
2. `.github/workflows/release.yml` triggers on tag push (`v*`) and rolling `nightly` on push to `main`.
3. Matrix builds produce binaries for `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`, `x86_64-apple-darwin`, and `aarch64-apple-darwin`.
4. Artifact packaging bundles stripped executable together with companion `skills/` (including `skills/install.sh`) into `.tar.gz` and produces `SHA256SUMS`.
5. `crates/forge-index/build.rs` configures `.std("c11")` and `.warnings(false)` for clean Tree-sitter compilation on Apple Clang.

---

### task: installer-script-and-client-setup

```yaml
task_ref: installer-script-and-client-setup
target: Develop zero-dependency POSIX install.sh with platform detection, skill placement, and .mcp.json client setup
proof_policy: seam-test-first
contract_revision: 4
scope:
- install.sh
- docs/reference/mcp-setup.md
- skills/install.sh
- src/cli/reload.rs
- tests/mcp.rs
- tests/mcp/installer.rs
status: completed
subtasks:
- subtask_ref: posix-platform-and-arch-detection
  title: Implement pure POSIX sh platform detection for Linux (x86_64, aarch64) and macOS (x86_64, arm64) with SHA256 verification
  status: completed
  evidence: 'Revision4 build claim50 installer-builder R6. Full cargo test --test mcp installer:: exit0: 13 passed/0 failed/0 ignored (/tmp/in001-r6-green-suite.log). Exact retained-hardlink runtime-red exit101 0 passed/1 failed before production repair (/tmp/in001-r6-red.log). Claim-bound rust-check-and-test scoped strict Clippy plus exact update_reexecutes_installed_image_retained_by_hardlink exit0 1 passed/0 failed (/tmp/in001-r6-verify-final.json). Both POSIX shell syntax checks and git diff --check exit0. BB36-38 and final cumulative decision record explain regression, identity repair and boundaries. Four Linux/macOS target selections, unsupported platform and checksum nonmutation remain covered by isolated real native archive/HTTP seams. Native Linux execution; foreign targets use controlled uname. No local native macOS/dash/wget proof.'
- subtask_ref: binary-placement-and-skill-sync
  title: Place binary into ~/.local/bin/ and execute bundled skills/install.sh to sync skills to ~/.agents/skills and agent dirs
  status: completed
  evidence: 'Revision4 build claim50 installer-builder R6. Full cargo test --test mcp installer:: exit0: 13 passed/0 failed/0 ignored (/tmp/in001-r6-green-suite.log). Exact retained-hardlink runtime-red exit101 0 passed/1 failed before production repair (/tmp/in001-r6-red.log). Claim-bound rust-check-and-test scoped strict Clippy plus exact update_reexecutes_installed_image_retained_by_hardlink exit0 1 passed/0 failed (/tmp/in001-r6-verify-final.json). Both POSIX shell syntax checks and git diff --check exit0. BB36-38 and final cumulative decision record explain regression, identity repair and boundaries. Atomic binary replacement, checksum nonmutation, skill synchronization/preservation remain green. R1 nonfile destination cases and R3 no-rsync aliased agent skill roots retained; R1-R3 independently confirmed BB31. No real user HOME writes.'
- subtask_ref: mcp-client-config-setup
  title: Configure or update .mcp.json in current project with contextunity-forge stdio server configuration
  status: completed
  evidence: 'Revision4 build claim50 installer-builder R6. Full cargo test --test mcp installer:: exit0: 13 passed/0 failed/0 ignored (/tmp/in001-r6-green-suite.log). Exact retained-hardlink runtime-red exit101 0 passed/1 failed before production repair (/tmp/in001-r6-red.log). Claim-bound rust-check-and-test scoped strict Clippy plus exact update_reexecutes_installed_image_retained_by_hardlink exit0 1 passed/0 failed (/tmp/in001-r6-verify-final.json). Both POSIX shell syntax checks and git diff --check exit0. BB36-38 and final cumulative decision record explain regression, identity repair and boundaries. Nested JSON, raw/escaped Unicode, owned fields and unrelated user settings preservation remain covered. Malformed JSON and nine invalid UTF8 byte variants reject without binary/config/skill mutation. SELF1 escaping distinction retained. Config symlink guard source-reviewed, no separate runtime symlink config proof.'
- subtask_ref: update-lifecycle-and-reload
  title: Implement seamless upgrade check and trigger contextunity-forge-mcp reload (SIGHUP) for running servers
  status: completed
  evidence: 'Revision4 build claim50 installer-builder R6. Full cargo test --test mcp installer:: exit0: 13 passed/0 failed/0 ignored (/tmp/in001-r6-green-suite.log). Exact retained-hardlink runtime-red exit101 0 passed/1 failed before production repair (/tmp/in001-r6-red.log). Claim-bound rust-check-and-test scoped strict Clippy plus exact update_reexecutes_installed_image_retained_by_hardlink exit0 1 passed/0 failed (/tmp/in001-r6-verify-final.json). Both POSIX shell syntax checks and git diff --check exit0. BB36-38 and final cumulative decision record explain regression, identity repair and boundaries. Real default and explicit server updates preserve PID/MCP stream and adopt new inode. New retained-hardlink old installed image test passes with literal deleted-suffix pathname absent and with different-inode foreign collision. Existing R5 foreign literal linked/hardlinked/unlinked table retains no-pending-HUP exclusions and stream survival. Process dev+ino vs literal-path dev+ino plus UID/exact path/canonical CLI/stable proc reread replaces zero-link necessity. Previous manual-restart hardlink exception withdrawn; acceptance unchanged. Identical inode/path aliases remain ambiguous and fail closed. macOS implementation unchanged; native compile/runtime proof remains open; native-gated seam retained.'
receipt:
  delivery_gate: deliver
  candidate_gate: build
  commit: 98a4f73b9dbdbe3a0e2d1d79609777dffd0ca07c
  contract_revision: 4
  passed_at: 2026-11-10T19:28:54.567201553+00:00
  evidence:
    command_proof:
      command: rust-check-and-test
      exit_code: 0
      log: |
        [rust-check-and-test::tests/mcp/installer.rs]
        SCOPE_VIOLATION_WARNING: docs/milestones/IN-001-installer-distribution-and-acdd-onboarding.md
        PASS rust-check-and-test::tests/mcp/installer.rs: exit=0 passed=1 failed=0 (10368 ms)
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Six admitted blobs exactly match accepted build snapshot221146c046c63ad6188f8920842ddc9f810fd010, rechecked after tests; no protected code/profile/config edits. Coordinator-owned milestone authority state is outside this candidate and untouched. Each scoped change supports installer acceptance or rev4 canonical reload seam.
        claims:
          applicable: true
          evidence: R1-R6 closure and limitations recorded BB40. Production POSIX installer, real checksummed archive and companion skills, config preservation/malformed UTF8/JSON, HOME quoting, pipe mode, path/symlink/failure negatives exercised. Independent actual R5 foreign suffix process unsignaled and R6 retained-hardlink default MCP reexecutes new inode with live tools/list. Installer13 + reload parser2 pass; strict all-targets/all-features Clippy exit0. Darwin ABI/source checked, native macOS execution explicitly unproved locally and not represented by Linux mapping/parser checks.
        concurrency:
          applicable: true
          evidence: Identity query checks same UID, exact typed Cli server invocation, live dev/inode and literal-path disambiguation; rereads process exe+identity after argument parsing. Private shell staging and same-directory atomic executable/config replacement preserve existing data on preflight/checksum/malformed input failures. Scope does not alter Forge claim/revision/receipt transactions or promise a whole-installer concurrent transaction. No source changes after candidate binding.
        project_isolation:
          applicable: true
          evidence: All runtime probes use isolated temporary HOME/project/config, verified local release fixtures and owned child processes; no live configuration or external signals. Existing foreign executable literal-suffix, nonserver CLI commands, skills alias and binary directory/symlink rejection verified. Archive/checksum selection and staged extraction preserve configured project/server data; no linked workspace/task resolution changes.
        administration:
          applicable: true
          evidence: No migration/profile/hook/credential/protected build edits. Review worker installer-reviewer differs from builder installer-builder, claim57 bound to contract4 and exact candidate. BB40 preserves finding origins, proof and native-platform boundary for delivery; automated Forge delivery will govern receipt/commit without manual receipt or source mutation.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ZERO-DEPENDENCY-INSTALL: The install.sh installer must run via standard POSIX sh using only curl/wget and tar, installing working binaries and invoking skills/install.sh without requiring cargo, rustc, or C compilers.'
    - 'INV-NO-SKILLS-IN-BINARY: Skills are pure companion assets, NEVER embedded into the Rust binary via build.rs codegen, include_bytes!, or internal Rust CLI installers. install.sh distributes and places companion skills into ~/.agents/skills/ directly from release archives or GitHub.'
    - 'INV-UPDATE-SEAMLESSNESS: The installer/updater must cleanly detect existing installations, preserve user-tailored configurations, update binary and skills without data loss, and execute contextunity-forge-mcp reload (sending SIGHUP) to seamlessly notify running MCP server processes.'
    - 'INV-AGENTIC-ONBOARDING: Repository onboarding and scaffolding is driven by agent skills, leveraging LLM context to generate tailored forge-mcp.yaml (with linked_workspaces), ACDD profiles, AGENTS.md, documentation structures, and initial milestones without bloating the Rust binary.'
    architectural_notes:
    - 'decisions: {"subject":"Installer architectural proof map final binding and builder handoff","architecture_map":10,"execution_notes":14,"resolves_blocker":15,"authority":{"contract_revision":3,"contract_claim_revision":10,"worker":"installer-contract","snapshot":"0b2a9a053ea5f1af29132cef0f33f1ad04507186"},"admission_and_archive":{"scope_admitted_by_coordinator":8,"producer_consumer_agreement":[6,7]},"verification":{"compile":{"command":"cargo test --test mcp --no-run","exit_code":0,"log":"/tmp/in001-installer-contract-compile.log","warnings":0},"runtime_red":{"command":"contextunity-forge-mcp task verify forge-mcp/m-installer-distribution-and-acdd-onboarding:installer-script-and-client-setup --test tests/mcp/installer.rs::verified_release_installs_real_binary_and_companion_skills_without_runtimes --timeout 1800 --format json","underlying_command":"cargo test --test mcp -- verified_release_installs_real_binary_and_companion_skills_without_runtimes","actual_exit_code":101,"tests_passed":0,"tests_failed":1,"configured_failure_signature":"assertion failed","matcher":"accepted: evidence non-null, error null, verification diagnostic_mode pass","observation":"/tmp/in001-installer-contract-verify-final.json"},"gate":"contract accepted; stage build ready"},"sensitivity":{"observed":"Missing install.sh fails the genuine native verified-release install scenario at its installation-status assertion; compilation succeeds. All seven domain test functions compile.","remaining":"The checksum mismatch/missing-asset, platform permutation, JSON malformed/escaped/nested preservation, and real Linux MCP update scenarios are authored but are not claimed executed by this one-test contract proof. Builder must run cargo test --test mcp installer:: after implementing, in addition to claim-bound exact test and strict Clippy."},"implementation_reminders":["Use FORGE_RELEASE_BASE_URL as overridable release asset directory; stable default and target names/layout in #6/#7","Existing skills/install.sh is Bash and requires POSIX conversion through its existing owner; no duplicated sync logic or embedded skills","JSON test includes escaped mcpServers/member keys, Unicode strings, nested objects and tailored Forge args/env/cwd. Preserve all user JSON except Forge command/type update","Checksum negative table rejects both bad bytes and a correct digest belonging only to another asset. Test fixture represents GitHub HTTP only; real binary, tar and skills installer stay in boundary","Linux update tests existing native executable inode replacement, actual installed serve reexec with surviving initialized MCP stream, bundled skill refresh, unrelated skill and forge-mcp.yaml preservation","Local sh is Bash; Ubuntu sh/dash runtime and source review still establish stricter POSIX grammar. Current production CLI reload discovery is Linux-only; document macOS manual restart limitation without inventing a Rust installer"],"forbidden_effects":"No production, profile, Cargo.toml/build.rs or manifest changes by contract author; no external HOME writes, publication, benchmark or unrelated process signals. Coordinator-admitted manifest edits remain intact."}'
    - 'decisions: {"decision":"installer build ready for independent review","self_review_closure":{"finding":"INSTALLER-SELF-1","blackboard":20,"state":"confirmed-fixed","proof":"Existing JSON preservation input now includes a carriage-return member name distinct from mcpServers; final 7-scenario behavioral suite passes."},"final_verification":{"behavioral_command":"cargo test --test mcp installer::","exit_code":0,"passed":7,"failed":0,"ignored":0,"log":"/tmp/in001-installer-build-all-final.log","gate_command":"contextunity-forge-mcp task verify exact contract function","profile_command":"rust-check-and-test: scoped strict Clippy then exact test","gate_passed":1,"gate_failed":0,"claim_revision":13,"observation":"/tmp/in001-installer-build-verify-final.json","syntax":"sh -n install.sh && sh -n skills/install.sh","diff":"git diff --check"},"proof_limits":"No native macOS or dash runtime available locally. Platform selection uses uname boundary permutations with genuine native binary; Linux update uses genuine initialized serve and reexec. Additional full all-targets Clippy was cancelled while waiting for a lock; claim-bound scoped strict Clippy is the accepted proof. Optional extra copy/dry-run shell check was blocked before execution by tool allowlist and is not claimed.","scope":"install.sh, skills/install.sh, docs/reference/mcp-setup.md; one existing JSON test input refined for direct decoder proof. Foreign coordinator/contract changes preserved.","review_attention":"JSON decoded key ownership, Unicode escapes, duplicate owned keys, malformed input nonmutation, symlink handling, checksum asset selection, POSIX companion behavior, actual installed-path reload."}'
    - 'decisions: {"repair_map":"BB22 candidate1f473231; same builder; R1-R3 required defects, origin pre-existing-missed relative to initial installer implementation, not caused by SELF1 repair","clusters":[{"ids":["IN001-R1"],"authority":"acceptance3/5","trace":"pre-existing destination directory or directory symlink -> install.sh final-path validation -> POSIX filesystem path shape -> staged rename -> reject before skill/config effects -> dedicated directory/symlink/fifo input test with target+config+skill preservation -> remove guard mutation reproduces old pass/corruption","callers":"single install.sh mv binary_stage binary owner; skill/config downstream. No concurrency/TOCTOU transaction promise."},{"ids":["IN001-R2"],"authority":"acceptance5; BB10 malformed JSON nonmutation","trace":"invalid UTF8 bytes -> existing POSIX awk parser boundary -> byte validation under LC_ALL=C -> client staging -> reject before binary/skill/config effects -> dedicated bytes table and existing valid Unicode inputs -> omit UTF8 validator mutation reproduces old acceptance","dimensions":"invalid leader/continuation, truncation, overlong, surrogate, >10FFFF; valid raw Unicode and escapes preserved; HOME command must itself encode valid UTF8"},{"ids":["IN001-R3"],"authority":"acceptance4 / update no data loss","trace":"agent skills root aliases canonical hub -> skills/install.sh sync_dir owner -> physical directory identity -> avoid self-destructive copy/delete -> dedicated real no-rsync alias case -> remove identity check reproduces canonical skill deletion","dimensions":"link/copy/dry-run/selection; same physical source/destination; separate existing regular folders still sync; no duplicate sync owner"}],"r4":"required integration defect at reload CLI; attempted extend_scope rejected TASK_SCOPE_INVALID outside roots. Coordinator updating authority; no source edit or installer discovery duplication.","verification":"Author compiling runtime-red behavioral regressions first. Coordinator requested no task verify/submit during authority amendment."}'
    - 'decisions: {"decision":"Admit the canonical reload CLI owner as necessary to fulfil existing update acceptance","reason":"Review #22 R4 reproduces an initialized generated no-subcommand MCP server remaining on its old inode after update. Existing args must be preserved; changing only newly generated args does not fix existing clients. Duplicate process discovery in install.sh would bypass the existing owner.","authority":"Acceptance 6 and INV-UPDATE-SEAMLESSNESS unchanged; only src/cli/reload.rs added to task scope; revision 4 synchronized, which returns to contract gate. Preserve independent tasks and accepted release/onboarding proofs.","evidence":"code_map impact depth 1, src/cli/mod.rs dispatch defaults None to Command::Serve; review #22 and /tmp/in001-proof-review-9y_m80fr/default-caller-reload-report.json","verification":"New runtime-red production seam, canonical CLI argument interpretation, same executable and UID restriction, avoid signalling other commands; existing macOS reload limitation must be evaluated against contract with evidence."}'
    - 'decisions: {"subject":"IN001-R4 contract restored; executable default-caller regression accepted","architecture_map":27,"review_finding":22,"origin":"pre-existing-missed canonical reload owner; R4 remains unresolved until production fix and independent verification","binding":{"contract_revision":4,"contract_claim_revision":27,"contract_snapshot":"677b20365738115aa4dea901e614cf172fbd6fad","stage":"build ready"},"test":"tests/mcp/installer.rs::update_reexecutes_default_configured_server_without_signaling_other_commands","proof":{"compile":{"command":"cargo test --test mcp --no-run","exit_code":0,"warnings":0,"log":"/tmp/in001-r4-contract-compile.log"},"red":{"underlying_command":"cargo test --test mcp -- update_reexecutes_default_configured_server_without_signaling_other_commands","exit_code":101,"tests_passed":0,"tests_failed":1,"duration_ms":13229,"configured_signature":"assertion failed","engine_observation":"evidence non-null; error null; diagnostic_mode pass; contract submit accepted","full_observation":"/tmp/in001-r4-contract-verify.json"}},"test_boundary":"Real installer first produces actual .mcp.json; real installed binary is launched with its command/args and project cwd. The resulting no-subcommand MCP process initializes and answers tools/list before genuine installer update. Existing literal-serve owner misses this server, so runtime-red reaches the 10-second default replacement-inode assertion. No source shim, dummy panic, or compiler error is used.","negative_boundary_refinement":"Actual same-binary CLI read serve is held on a small stdout socket after beginning its production read response. It and an initialized foreign-executable-path MCP process are SIGSTOPped using only owned PIDs. On Green, both SigPnd and ShdPnd must contain no SIGHUP before SIGCONT; the CLI then finishes successfully and the foreign MCP stream answers. This proves exclusion even if a wrongly signaled foreign server could reexec the same inode. These post-fix assertions are authored and compiled but not claimed reached by the old-owner Red.","preservation":"Builder R1–R3 edits preserved. Contract author changed only tests/mcp/installer.rs; no src/cli/reload.rs/profile/manifest edits. Existing explicit-serve update regression remains.","platform_authority":"Runtime proof is Linux only. macOS is supported by installer acceptance2 and active-instance reload remains acceptance6; current non-Linux source errors. Native macOS unavailable here; no native proof, no inferred acceptance exception, and no closure claim for the macOS gap.","handoff":"Coordinator and same installer-builder notified immediately after accepted contract. Builder owns canonical reload classification/platform implementation, required green and strict checks, and same-reviewer closure."}'
    - 'decisions: {"r4_owner_map":{"authority":"rev4 scope + parent authorization Linux/macOS discovery; BB27","origin":"pre-existing-missed canonical reload predicate exposed by default installer caller","trace":"installed command+args -> platform process enumeration -> same UID + authoritative executable path -> exact canonical Cli grammar None/Serve -> SIGHUP -> existing Unix serve_session exec/session resume","linux":"Existing /proc executable/user checks retained; NUL argv parsed with Cli::try_parse_from, help/version/malformed/non-server excluded","macos":"Existing libc APIs only: proc_listallpids inventory, proc_pidinfo effective UID, mandatory exact proc_pidpath, KERN_ARGMAX/KERN_PROCARGS2 bounded buffer and argc parser; any identity/argv failure skips candidate; no saved-path or argv0 executable fallback","mutation_matrix":["literal serve-only predicate: default live MCP remains old inode (contract runtime-red confirmed)","select all/any serve token: same-binary read serve receives HUP (contract real negative)","remove executable fence: foreign native MCP receives pending HUP (contract real negative)","omit reload/nonatomic replace: live executable inode/session assertion fails","macOS missing/noop discovery: native kqueue NOTE_EXEC absent for both supported invocations; pending native CI execution"]},"local_result":"cargo test --test mcp installer:: exit0 11 passed/0 failed/0 ignored before final formatting; real revised R4 included","native_test":"macos_update_executes_default_and_explicit_servers_with_confined_discovery cfg macos; real installed executable + generated default and global-root explicit serve + kqueue NOTE_EXEC + preserved streams; foreign executable and real nonserver read serve excluded","proof_limit":"Linux host, only Linux Rust target installed. No local macOS compile/runtime proof. Mandatory proc_pidpath after unlink/replace remains tested only by future native CI, no weaker identity fallback.","sources":["https://raw.githubusercontent.com/apple-oss-distributions/adv_cmds/main/ps/print.c KERN_PROCARGS2 layout","https://raw.githubusercontent.com/apple-oss-distributions/xnu/main/bsd/kern/proc_info.c executable vnode path","installed libc Apple FFI declarations"],"remaining":"Final formatting tests, claim-bound verify, closure ledger and independent review. No source edits after eventual build submit."}'
    - 'decisions: {"title":"Cumulative installer repair ledger and independent review admission","authority":{"review":22,"contract_revision":4,"claim_revision":30,"worker":"installer-builder","r4_contract":27},"lineage":[{"id":"INSTALLER-SELF-1","origin":"introduced-regression in original JSON decoder; fixed before first candidate","disposition":"locally-confirmed-fixed","evidence":"Escaped control-member key preserved in real installer JSON table; final suite green."},{"id":"IN001-R1","origin":"pre-existing-missed in first installer candidate","disposition":"locally-confirmed-fixed; independent closure pending","owner":"install.sh final-path guard","invariant":"Only absent/regular-file final executable destinations reach publication; directory aliases and special files cannot redirect staged executable/config/skills mutation","trace":"acceptance3/5 -> installer -> POSIX destination type -> no downstream writes -> real directory/symlink table asserts config/skills/target bytes and no leaked stage","red":"pre-fix dedicated test runtime fails accepting directory; reviewer reproduced both topology variants","green":"nonfile_binary_destinations_preserve_configuration_and_skills","mutation":"Remove destination guard restores accepted-directory publication; pre-fix source is concrete mutation."},{"id":"IN001-R2","origin":"pre-existing-missed in first JSON parser","disposition":"locally-confirmed-fixed; independent closure pending","owner":"install.sh POSIX awk boundary","invariant":"Only valid UTF8 reaches grammar parse and owned-field rewrite; all rejected config bytes and installation state survive","trace":"acceptance5 -> C-locale UTF8 byte grammar -> JSON grammar -> staged client -> binary/skills/config; raw and escaped valid Unicode preserved","red":"pre-fix dedicated test accepts FF and fails runtime; independent review observed overlong and surrogate acceptance","green":"invalid_client_utf8_preserves_all_installation_state: 9 byte variations plus existing valid Unicode/escaped-key preservation suite","mutation":"Omit UTF8 validator call restores original FF/overlong/surrogate acceptance."},{"id":"IN001-R3","origin":"pre-existing-missed in existing destructive fallback, retained by POSIX conversion","disposition":"locally-confirmed-fixed; independent closure pending","owner":"skills/install.sh canonical sync_dir","invariant":"Physically identical source/destination directories never enter rsync or destructive copy fallback","trace":"acceptance4/update preservation -> packaged companion -> canonical physical path identity -> no-rsync fallback -> real aliased agent-root install/user skill readback","red":"pre-fix alias test exits1 after removing source through alias","green":"aliased_agent_skill_root_preserves_the_canonical_hub_without_rsync","mutation":"Remove same_dir fence re-enters original destructive self-sync. README self-copy shares same identity helper."},{"id":"IN001-R4","origin":"pre-existing-missed canonical reload literal-token filter","disposition":"Linux locally-confirmed-fixed; macOS implementation authored/native proof pending; independent review pending","owner":"src/cli/reload.rs","invariant":"Only canonical default/explicit server modes with exact executable/user identity receive SIGHUP","trace":"acceptance6 -> generated/existing command+args -> platform native inventory/identity/argv -> Cli::try_parse_from None/Serve -> single shared signal loop -> existing Unix session-preserving exec","red":"rev4 accepted runtime-red exact default-configured-server test; observation /tmp/in001-r4-contract-verify.json","green":"Both real Linux update tests pass; default test excludes actual same-binary read serve and foreign-executable MCP via pending-HUP assertions. Pure canonical grammar and Darwin argc/padding/empty-argument decoder tests 2/0.","mutations":"Literal serve-only selector leaves default old inode; all/any-token selector signals real read serve; absent path fence signals foreign executable; omitted reload fails inode/stream assertions. macOS no-op discovery fails native kqueue NOTE_EXEC test when executed."}],"final_checks":{"suite":"cargo test --test mcp installer::; exit0 passed11 failed0 ignored0; /tmp/in001-installer-repair-suite-final.log","parser":"cargo test --lib cli::reload::tests::; exit0 passed2 failed0 ignored0; /tmp/in001-reload-parser-tests.log","gate":"CLI task verify exact update_reexecutes_default_configured_server_without_signaling_other_commands; rust-check-and-test scoped strict Clippy+exact test; exit0 passed1 failed0; /tmp/in001-installer-repair-verify-final.json","syntax":"sh -n both installers, git diff --check: exit0"},"residual_and_impact":"Fresh Forge reload.rs impact binds current inventory and confirms CLI module owner; ast-grep reload::run() one production dispatch and libc::kill(pid,SIGHUP) one canonical signaling call. Forge shell source reads confirm one CLI reload consumer, same_dir fence before sync effects and UTF8 call before durable effects. Shell owners lack indexed graph nodes, explicitly limited rather than absent.","native_boundary":"No native macOS host/toolchain target locally. macOS libproc/sysctl implementation, exact executable/uid/CLI confinement and kqueue real-process test are authored; actual Mac compilation/path behavior after atomic rename require native CI evidence. Pre-existing unused_mut possibility in another owner not edited or claimed fixed.","review_admission":"All IDs accounted; final source edits complete; forthcoming build snapshot binds exact candidate for same independent reviewer. No publication, external HOME writes, benchmarks or installer process-discovery duplication."}'
    - 'decisions: {"finding":"IN001-R5","review":31,"origin":"pre-existing-missed inherited Linux textual deleted marker, retained through R4 repair; R1-R3 now independently confirmed-fixed","authority":"BB27 frozen exact executable/user confinement; acceptance6","owner":"src/cli/reload.rs Linux matching_servers","closure_trace":"Existing live executable literally named installed-path plus deleted marker -> /proc exe readlink string -> must validate deleted identity before suffix normalization -> canonical Cli/uid checks -> SIGHUP -> real stopped foreign process must have no pending HUP","adjacent_matrix":["regular installed same-path process remains selectable","genuinely unlinked old installed image after atomic replace remains selectable/default and explicit stdio survival","existing foreign literal deleted-suffix image, linkcount1, must be excluded","unlinked foreign literal deleted-suffix image has double kernel+literal suffix and must remain excluded","macOS path/UID/argv branch unchanged"],"regression":"Dedicated real-process Linux test first: real copied foreign literal-suffix native server initialized, stopped, canonical installed reload invoked, pending-HUP and stream/executable readback inspected. Runtime-red before production edit.","policy":"Use authoritative /proc executable metadata to distinguish vanished installed inode from still-linked literal filename; no text-only exception, no broadened process scope. Exact implementation must preserve current positive update seams; source reads and kernel path semantics reconciled before edit."}'
    - 'decisions: {"finding":"IN001-R5","authority":"review31; rev4 buildclaim40","red":"/tmp/in001-r5-red.log: genuine native foreign live literal-suffix process received pendingHUP; runtime101,0passed/1failed","green":"cargo test --test mcp installer:: /tmp/in001-r5-green-suite.log: exit0,12passed/0failed/0ignored; includes foreign linked/hardlinked/unlinked literal suffix and accepted explicit/default live update inode+stream positive assertions","repair":"Linux marker accommodation now requires executable metadata.nlink()==0 and stable proc symlink reread before stripping marker. Direct exactpath/UID/canonical CLI, shared signal loop and entire macOS code remain unchanged.","hardlink_limit":"IMPORTANT: an old installed executable inode retained through other hard links is deliberately skipped under ambiguous marker matching. Existing hardlinked installations may need manual MCP server restart after update. This is fail-closed identity policy, not unconditional hardlink update coverage or an acceptance amendment. Limit is now prominently documented in docs/reference/mcp-setup.md.","closure":"R1-R3 independently confirmed-fixed in31. R4 Linux caller selection confirmed; its process isolation residual R5 now locally repaired pending same independent reviewer. Native macOS compile/runtime remains open as31; no new Darwin claim. SELF1 prior closure retained.","mutation":"Remove zero-link guard restores original same-name foreign false signal (runtime-red concrete pre-fix); strip repeatedly would admit unlinked literal double suffix (new permanent row); ordinary/default/explicit update tests reject disabling legitimate zero-link accommodation.","residual":"Fresh Forge impact+AST scan requested for canonical Linux marker owner; source diff against527ad39 affects only Linux matching_servers, no macOS/shared classifier edits.","remaining":"Claim-bound exact R5 strict-Clippy proof running, then snapshot/subtask evidence and independent review; no source edits after submit."}'
    - 'decisions: {"finding":"IN001-R6","origin":"introduced-regression by R5 zero-link requirement (before527ad39 and after5ea71c4 native reports in36). R5 itself confirmed fixed but canonical identity cluster unresolved.","authority":"Coordinator explicitly resumes same owner, unchanged acceptance6/INV/BB27. Withdraw manual-restart exception; do not scope around failed behavior.","owner":"src/cli/reload.rs Linux matching_servers; docs setup removes narrowed claim","ordered_trace":"real generated default configured server retained by hardlink -> real installer atomic replacement -> /proc exe link + authoritative process inode -> compare dev/ino of literal candidate pathname -> normalize marker only when literal pathname does not identify running image -> same UID/canonical CLI -> SIGHUP -> real same-PID replacement inode/initialized stream","identity_policy":"Exact path must identify current executable inode; suffix case must strip exactly once to installed path, and literal pathname metadata must either be NotFound or a different dev+ino from running proc executable. Same identity or query ambiguity rejects. Recheck proc symlink+inode snapshot; no nlink-zero necessity. Same filesystem identity preserved; macOS unchanged.","full_matrix":{"positive":["own current normal linked validserver","own replaced normal zero-link validserver(existing default+explicit update)","own replaced normal retained-hardlink validserver(new dedicated full installer table)","retained own old inode plus live foreign literal-marker path with different inode(new collision row)"],"negative":["foreign ordinary path validserver(existing stopped-process R4)","foreign literal-marker linked/hardlinked/unlinked validserver(existing stopped-process R5 table)","own same executable nonserver with serve argument(existing R4 actual read)","same marker string and identical literal-path/proc dev+ino must reject","identity stat errors other than NotFound and changed proc identity fail closed"],"limits":"No new PID-reuse/namespace/TOCTOU transaction guarantee; native macOS evidence still open. Inherently ambiguous same-inode alias at literal marker pathname rejected rather than misidentified."},"regression":"Dedicated test first on5ea71c4: actual install/config, hardlink live installed image, native initialized server, actual verified update. Table absent vs different-inode literal-path collision and excluded foreign signal state; runtime red before canonical production edit.","closure_admission":"Repair complete owner cluster, retain every existing topology negative. Review31/36 confirmed SELF1/R1-R3; R4/R5/R6 reconcile as one process identity cluster. No new review until final bound evidence and cumulative ledger."}'
    - 'decisions: {"finding":"IN001-R6","authority":"BB36 typed rejection; implementation map BB37 and red/repair BB38","classification":"R5 introduced regression repaired; independent confirmation pending","policy":"Linux selection compares authoritative process dev+ino against literal proc-exe pathname metadata. Genuine deleted old installed images with retained hardlinks are eligible when literal pathname is absent or identifies a different inode; literal pathname identifying the running image or ambiguous metadata errors fail closed. Direct path requires current executable identity. Same-device, exact canonical path, UID, canonical CLI parser and stable proc link/image recheck remain enforced. No nlink-zero necessity; macOS and shared parser unchanged.","coverage":"Own installed regular update and retained-hardlink update, with literal suffix absent and different-inode collision; foreign literal suffix linked/hardlinked/unlinked remains excluded through pending-HUP and stream assertions. No new PID-reuse/namespace/transactional TOCTOU guarantee.","proof":{"red":"/tmp/in001-r6-red.log: compiling runtime assertion failure exit101, 0 passed/1 failed","suite":"/tmp/in001-r6-green-suite.log: cargo test --test mcp installer:: exit0, 13 passed/0 failed/0 ignored","bound":"/tmp/in001-r6-verify-final.json: claim50/revision4 rust-check-and-test scoped strict Clippy + exact retained-hardlink test exit0, 1 passed/0 failed","checks":"git diff --check; sh -n install.sh; sh -n skills/install.sh exit0"},"cumulative":"SELF1 and R1-R3 repairs retained (R1-R3 independently confirmed BB31); R4 Linux default and explicit reload retained; R5 linked/hardlinked/unlinked literal suffix negatives retained. BB34 manual-restart hardlink exception and claim40 lifecycle evidence are withdrawn and superseded: ordinary retained-hardlink installed servers update seamlessly. Acceptance unchanged. Identical literal-path inode aliases remain ambiguous/fail closed.","boundary":"Native macOS compile/runtime not available on local Linux-only target; authored macOS-gated production seam remains for native CI. No local native macOS/dash/wget proof claimed. Parent manifest edits preserved."}'
    - 'decisions: {"subject":"Independent revision-4 closure: inode-based reload identity restores own hardlink updates and excludes literal suffix processes","candidate":"221146c046c63ad6188f8920842ddc9f810fd010","contract_revision":4,"claim_revision":57,"decision":"pass","finding_lineage":[{"id":"IN001-R1","origin":"original implementation","disposition":"closed","evidence":"Existing regular executable or absent target only; directory and directory-symlink reject before configuration/skills mutation; production shell isolated negative rows pass."},{"id":"IN001-R2","origin":"original implementation","disposition":"closed","evidence":"POSIX JSON parser rejects original invalid raw UTF-8 and boundary encodings before durable install changes; malformed input remains byte-exact. Companion production package replay and current installer suite cover negatives and valid Unicode."},{"id":"IN001-R3","origin":"original implementation","disposition":"closed","evidence":"Real companion skills installer recognizes canonical hub/agent alias before copy fallback; current no-rsync alias seam preserves hub plus local unrelated data."},{"id":"IN001-R4","origin":"inherited default invocation missed","disposition":"closed","evidence":"Canonical Cli parser recognizes no-subcommand and explicit serve only; exact generated default-config MCP stream reexecutes after update; nonserver commands containing serve are excluded."},{"id":"IN001-R5","origin":"pre-existing-missed Linux deleted suffix ambiguity","disposition":"closed","evidence":"Direct executable path requires current dev/inode; suffix interpretation checks exact literal path identity rather than unconditional suffix removal or link count. Independent original literal-suffix native process remains unsignaled: /tmp/in001-reload-suffix-8xi1rc04/report.json."},{"id":"IN001-R6","origin":"introduced-regression in 5ea71c4 R5 nlink remediation","disposition":"closed","evidence":"No link-count exclusion remains. Actual install->hardlink->default MCP->update reexecutes old live image despite nlink2; installed/running inode314123543, retained314123402, tools/list preserved: /tmp/in001-hardlink-update-_rtp666e/report.json. New permanent seam also checks different-inode literal-name collision remains unsignaled."}],"identity_analysis":"Linux discovery binds effective UID, exact executable or one kernel deletion marker interpreted with live dev/inode evidence, and canonical CLI grammar. Executable target and identity are reread after parsing before returning a match. Same-inode literal alias remains intrinsically ambiguous and fails closed; no PID-reuse/namespace/whole-install transaction guarantee is introduced. Source/AST confirms one shared SIGHUP site and unchanged CLI reload dispatch.","scope_binding":"All six admitted file blobs rehashed equal 221146c046c63ad6188f8920842ddc9f810fd010 after verification. Coordinator-owned milestone manifest is separate authority state and was not edited by reviewer. No source edits, live client settings, publications or unrelated processes touched.","verification":{"installer_suite":"cargo test --test mcp installer:: => 13 passed,0 failed,0 ignored","reload_parser":"cargo test --lib cli::reload::tests:: => 2 passed,0 failed","lint":"cargo clippy --all-targets --all-features -- -D warnings => exit0","logs":["/tmp/in001-review-r6-suite.log","/tmp/in001-review-r6-parser.log","/tmp/in001-review-r6-clippy.log"],"source_checks":"sh -n both installers; git diff --check; changed-owner duplicate detection found no clones; integration/impact and signal/dispatch AST inspected"},"proof_boundary":"Linux native production process and real POSIX shell/archive/skills/config paths executed in isolated /tmp HOME. macOS implementation is unchanged from companion ABI/source review and parser tests; authored native macOS process/kqueue test has NOT been compiled or run on this Linux host. Native Darwin runtime proof remains explicitly separate; Linux uname mapping is asset selection evidence only."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. `install.sh` executes in pure POSIX `sh` without external language runtimes or compiler tools (`curl -fsSL ... | sh`).
2. Architecture and OS detection accurately handles Linux (`x86_64`, `aarch64`) and macOS (`x86_64`, `arm64`), downloading the correct archive and verifying SHA256 checksum.
3. Binary atomically placed in `~/.local/bin/` with `chmod +x`, warning if `PATH` lacks the directory.
4. Executes bundled `skills/install.sh` to populate `~/.agents/skills/` and symlink/copy into detected agent directories (`~/.claude/skills/`, `~/.codex/skills/`, `~/.gemini/config/skills/`, `~/.cursor/skills/`).
5. Configures or updates `.mcp.json` with `contextunity-forge` stdio configuration without corrupting existing client configurations.
6. Trigger `contextunity-forge-mcp reload` (SIGHUP) if an existing instance is active.
7. Negative Invariant: ZERO Rust compilation or Rust tooling required to run the installer.

---

### task: acdd-onboarding-agent-skill

```yaml
task_ref: acdd-onboarding-agent-skill
target: Develop agentic onboarding skill for workspace discovery, adapter and linked workspaces configuration, ACDD profiles, and milestone synthesis
proof_policy: seam-test-first
contract_revision: 4
scope:
- skills/contextunity-forge/
- tests/mcp/skill_sync.rs
status: completed
subtasks:
- subtask_ref: onboarding-workflow-definition
  title: Author skills/contextunity-forge/references/onboarding.md detailing the full agent-driven repository onboarding lifecycle
  status: completed
  evidence: 'Implemented in skills/contextunity-forge/references/onboarding.md and entrypoint route. Validated by claim-bound scoped seam proof, installed-layout link regression and quick_validate; behavioral review boundary recorded in Blackboard #1820.'
- subtask_ref: adapter-and-linked-workspaces-generation
  title: Establish prompt patterns and templates for tailored forge-mcp.yaml generation with roots, ignores, and linked_workspaces
  status: completed
  evidence: 'Implemented in skills/contextunity-forge/references/onboarding.md and entrypoint route. Validated by claim-bound scoped seam proof, installed-layout link regression and quick_validate; behavioral review boundary recorded in Blackboard #1820.'
- subtask_ref: profile-and-agents-guidance
  title: Provide guided synthesis for .forge/acdd/profile.yaml command bindings and project-tailored AGENTS.md rules
  status: completed
  evidence: 'Implemented in skills/contextunity-forge/references/onboarding.md and entrypoint route. Validated by claim-bound scoped seam proof, installed-layout link regression and quick_validate; behavioral review boundary recorded in Blackboard #1820.'
- subtask_ref: documentation-and-milestone-consulting
  title: Provide methodology and templates for bootstrapping docs/ (architecture, adr, invariants) and initial milestone contracts
  status: completed
  evidence: 'Implemented in skills/contextunity-forge/references/onboarding.md and entrypoint route. Validated by claim-bound scoped seam proof, installed-layout link regression and quick_validate; behavioral review boundary recorded in Blackboard #1820.'
receipt:
  delivery_gate: deliver
  candidate_gate: build
  contract_revision: 4
  passed_at: 2026-11-10T17:24:15.346028750+00:00
  evidence:
    command_proof:
      command: rust-check-and-test
      exit_code: 0
      log: |
        [rust-check-and-test::tests/mcp/skill_sync.rs]
        SCOPE_VIOLATION_WARNING: docs/milestones/IN-001-installer-distribution-and-acdd-onboarding.md
        PASS rust-check-and-test::tests/mcp/skill_sync.rs: exit=0 passed=1 failed=0 (1182 ms)
      tests_passed: 1
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate e98100e5 differs from reviewed 66c796f only by repaired manifest example in admitted skills/contextunity-forge/. All four changed skill/test worktree blob identities match candidate. No source/configuration/protected-path scope expansion; git diff --check clean.
        claims:
          applicable: true
          evidence: 'Acceptance route and end-to-end onboarding guidance verified against live examples/schemas. IN003-R1/R2 from Blackboard #1 are confirmed-fixed in candidate example blob 84b069c: real CLI MILESTONE_VALID 2 tasks, 3 role mutations and 6 missing-title mutations rejected, supported milestone init/--in-place comments source-verified. Strict scoped Clippy and skill_sync 2/0/0 pass; prior verification content is identical to this candidate.'
        concurrency:
          applicable: true
          evidence: No concurrency runtime algorithm changed. Guide preserves independent review, immutable candidate and workflow-guidance authority. Candidate recapture is bound to accepted build e98100e5 with no intervening file edits. Earlier candidate rejection was accepted before fresh build binding; no stale proof treated as current.
        project_isolation:
          applicable: true
          evidence: Controlled isolated Python repo validates adapted profile/adapter/planned milestone, linked graph, representative symbol/docs search and one passing test. Existing settings/instructions preserved, linked sibling unchanged and milestone unstarted. Guide grants no sibling writes or shared lifecycle from linking alone.
        administration:
          applicable: true
          evidence: Companion-only skill change; no Rust CLI scaffolder, embedded assets, runtime profile/schema or permission changes. Guide preserves installation, client, agent-home, protected-branch and publication authorization boundaries. CLI validation is distinguished from test proof and domain reasoning. No benchmarks; runtime performance paths unchanged. Duplicate scan of changed test owner found zero groups.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-ZERO-DEPENDENCY-INSTALL: The install.sh installer must run via standard POSIX sh using only curl/wget and tar, installing working binaries and invoking skills/install.sh without requiring cargo, rustc, or C compilers.'
    - 'INV-NO-SKILLS-IN-BINARY: Skills are pure companion assets, NEVER embedded into the Rust binary via build.rs codegen, include_bytes!, or internal Rust CLI installers. install.sh distributes and places companion skills into ~/.agents/skills/ directly from release archives or GitHub.'
    - 'INV-UPDATE-SEAMLESSNESS: The installer/updater must cleanly detect existing installations, preserve user-tailored configurations, update binary and skills without data loss, and execute contextunity-forge-mcp reload (sending SIGHUP) to seamlessly notify running MCP server processes.'
    - 'INV-AGENTIC-ONBOARDING: Repository onboarding and scaffolding is driven by agent skills, leveraging LLM context to generate tailored forge-mcp.yaml (with linked_workspaces), ACDD profiles, AGENTS.md, documentation structures, and initial milestones without bloating the Rust binary.'
    architectural_notes:
    - 'decisions: {"decision":"independent-review-closure","candidate_snapshot":"e98100e5c0ca3c3adcb171157cfd064b8eec647e","prior_candidate":"66c796f32b2ce67bc229b1eeb36d200cebadbfee","binding":"Snapshot diff contains only the reviewed manifest example repair. All four changed skill/test worktree blobs match e98100e5 exactly, including repaired example 84b069c988658995e54cd3a6996fd7e5b03cfa85; no other candidate content changed.","finding_closure":[{"id":"IN003-R1","origin":"pre-existing-missed","disposition":"confirmed-fixed","evidence":"Copied repaired manifest validates MILESTONE_VALID with 2 tasks; original fails. Reintroducing role at each of 3 repositories and removing title at each of 6 subtasks all fail real CLI validation."},{"id":"IN003-R2","origin":"pre-existing-missed","disposition":"confirmed-fixed","evidence":"Comments now name supported milestone init <manifest.md>, required-skill preflight and --in-place worktree behavior; confirmed against live help and preflight_milestone_init_inner source."}],"owner":"skills/contextunity-forge/references/acdd-profile-examples/milestone-manifest-with-skills.md.example","proof_boundaries":"Strict scoped Clippy and skill_sync 2 passed/0 failed/0 ignored support unchanged Rust source. Structural seam proves route/assets, not arbitrary agent decisions. Controlled Python-repository execution confirms adapted profile/adapter/planned milestone, linked overview, source/doc lookup and one passing project test; preserves settings/instructions/sibling and leaves milestone unstarted. It is not an autonomous LLM forward test, complete ACDD lifecycle, or negative command-proof.","evidence_files":["/tmp/in003-review-repair-ycbjn8uh/report.json","/tmp/in003-review-behavior-pzxewnio/report.json"],"remaining_findings":[],"review_effects":"Read-only, no source edits. No benchmark, external installation, publication, profile changes or agent-home writes."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Acceptance

1. Author `skills/contextunity-forge/references/onboarding.md` and route it through `skills/contextunity-forge/SKILL.md` as the authoritative onboarding guide.
2. The skill guides any AI agent (Antigravity, Codex, Claude, Cursor) through end-to-end repository onboarding:
   - Inspecting repository topology, language manifests, and build tooling.
   - Authoring project-tailored `forge-mcp.yaml` with appropriate `roots:`, `ignore:`, and `linked_workspaces:` for sibling repositories.
   - Synthesizing `.forge/acdd/profile.yaml` binding project-specific verification commands and gate definitions.
   - Authoring `AGENTS.md` establishing mandatory Forge MCP navigation and project-level invariants.
   - Consulting on documentation structure (`docs/architecture/`, `docs/adr/`, invariants).
   - Proposing initial milestones and plans for iterative project delivery.
3. Negative Invariant: ZERO Rust compilation or Rust CLI code required. All onboarding capabilities are authored as reusable, evolvable agent skill instructions.
