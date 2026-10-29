---
id: AC-001-hierarchical-acdd-gates-and-milestone-orchestration
title: Hierarchical ACDD gates, modular catalogs, and milestone orchestration
doc_type: contract
status: completed
depends_on:
- m-milestone-handoff-pruning-evidence-cleanup-and-list-guidance
owners:
- src/core/tasks/
- src/engine/tasks/
- src/db/tasks_store/
- src/db/reader.rs
- tests/acdd/
- src/cli/
- skills/contextunity-forge/
- docs/
invariants:
- 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
- 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
- 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
- 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
- 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
- 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
- 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
- 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
- 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
- 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
- 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
- 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
- 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
- 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
- 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
- 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
- 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
- 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
- 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
- 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
required_skills:
- contextunity-forge
- agent-instructions
related_plans: []
started_at: 2026-10-24T02:28:16+00:00
repository: forge-mcp
project: forge-mcp
handoff:
  completed_at: 2026-10-29T13:29:26.115615988+00:00
  duration: 131h 1m
  commit: e877f3347edaebe931dc8b6d6aa05924ed99a54b
  target_base: 2e63013282c8d97b12b4209841ef34c70e27aa0c
  integration_evidence:
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: AC-001-hierarchical-acdd-gates-and-milestone-orchestration
      authority_digest: a7886c94550d43924945adb2f3dba6ffc722d3681c9a9b842fdfe41bf9e161cb
      implementation_digest: 7c7999098596b11c357ff089ad5e2c2f7129ed37f372eb5f128a1ce87c833cb0
      target_base_digest: 866dcbfad98548aa4a923ab7e6ec4be0d5c257e6e3b9bca6722b7b63bec6986e
      builders:
      - codex-ac001-builder
      - codex-ac001-synthesis-builder-r4
      - codex-ac001-task1-builder
      - codex-ac001-task2-recipient-builder
      - codex-ac001-task3-builder-20261024
      - codex-ac001-task6-builder-20261025
      - codex-ac001-task7-20261025
      - codex-sol-builder
    target_base: main
    review:
      worker_id: codex-ac001-independent-reviewer
      decision: pass
      summary: Milestone AC-001 implementation independently reviewed against target base main (2e630132). All 8 tasks delivered with accepted proofs, full test coverage across 860 passed tests and strict Clippy with zero warnings.
      scope_findings: []
      contract_findings: []
    verification:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 860
      tests_failed: 0
  proof_shas:
  - 081805f9a5e4a7ea6d3217de40f193da8af76b09
  - 11da4f59a86801bf5b97e9a6505419e58434c81a
  - 155ed37f3d9bd0790c1cb139feabe8f05928a159
  - 1612ba609cf66ab7c08ac767a6d40280061b9d47
  - 1a000bc6d1a9774854fb7e93f36b659166b2f382
  - 205fe8b9d21202a48a45afe1f1b0a97e6cbad893
  - 218d0753f0c7ed32b9ad7ccd29b34b8f94ae1489
  - 25e77d96c4703fbec0c71722d53322fb015e3348
  - 2e63013282c8d97b12b4209841ef34c70e27aa0c
  - 3448236d485f42f47db1a6ad02d15f7fd507840e
  - 397468a10cfc149aec471d10940ea77e84535885
  - 3a0ccb23c3cf0101b1023b53c6f2f76950b53b99
  - 3d30822b78544a627df91842ec919474ed098e0f
  - 3de0a94d1bb25ed9358556505eaa7bb7225a69e8
  - 43f6054a87d491e058bc1b7ce632a2fc0a2131fc
  - 45cbd49ba6b107234b71f0f719859d4d94081e86
  - 45d6725f64d715b3ea7b4e6efa69ce8e39ad952f
  - 46b61e686aa89e8b89c97e0c26e2addfc0517a96
  - 4c78414680da53df8514903b3ba8d66597cc4b10
  - 4d35ac4552dd948e9736e175c05d4f42fceab077
  - 51e2bc1183d555cc54b15c3629503dc3dc1234b9
  - 566c92e4a6f6e578a32db76cbdb2e457bdc7110f
  - 5c597ee494865187d6754246a319b5f0e520ac7d
  - 5dbbcbec22848d936b0d6af2217a40055cfafcea
  - 5df0f64b7149ee07ccd3de390075456ec08defe5
  - 5ffc87a263f57bacbde421a36df2b2a982d08ac0
  - 60418b2c99d106822aaba7dd9b934796fbdbaab9
  - 6047fabd10626ad3b40802fb28da05346a3522b5
  - 634a743aa6e7b07cc1a61012e92d39f0372b61df
  - 63a243dd90b591f4790556ab62cfbe643b2e8cc9
  - 6ca34eb5368753c7cfe84d5f97df9fb274f4fc7d
  - 6f34be590e564b136ab3fe5805ab6b83c8afe39c
  - 6f4d9800b74ecb8d7fe94cbda224522e2c148baa
  - 725c7769f8b619bd225279e37c5a3e3df667f95f
  - 7bb7453b5f092e3595945acd01124f087e2a0d8e
  - 835f749c9d9c3e7e5d319839df471770b27caf33
  - 8398442b8ba2d25e0c9a3b99038e09ab9c069bf1
  - 84c60237a02da005be83eec141d676e0a1d9428c
  - 8519cd6bee3985646a37ed69d551f0e7a19b66cb
  - 88b5d585572c9dcc24e989987115a318f40f3966
  - 88c081aa07ab09167ad5132da8e86a25e689acc5
  - 8a36c361dd93bd17e570a889e7327b1bc11e8f68
  - 8a9d7166884bef8747dadc304c7a6a4f70a54104
  - 8cc27c77ab38805e42af595d6150d0968f729899
  - 9304ad99e117d06bbf87a4ee725122c76ff0d498
  - 957e8d49b27d8ee0a076d36241b2dcd2a26b38a5
  - 9b0dfce476c0a312007a9bba06c21cda3604cda1
  - 9f1543c2d65999763749c7384ffbcc4f42bc72e0
  - a5561c0031208fbd2acc77c8470f6d37a3fc9b41
  - a9687e1f0466f730580d3cd0b2f06a7e33839981
  - ad339da22a33bf1feb7e5ee479ec5940e048fa8c
  - b6faaa83179917abb740a80b664a9f17e912e2de
  - b93f1604fec804e4e9f2fe1ff5cab68d9a14b9cd
  - c1de5f2aea48afba9dc48ebd750f4fd291ca0619
  - c225d615f063c27d1041bccf1172fda773a4188f
  - cc8059253ef79f985f5506c775dd899250c499fc
  - cffec4986f4320e44c7753550ae7c4557cf6301e
  - d1b70971043ed4f1e9ff0b8ebb4e4160b5c3980f
  - d2be9bf9e982e4e6cf05a00665315665071c49f4
  - d385a1696fddf2ca3b9b6009e0946bec76441ef9
  - d5f5dbb8c9d21b952207d29ac70dd0201fbe038b
  - d86812ee6d56368790ebe90f58764023333ce28c
  - d9a5c275f2331034bcb33360a338ca28cd4d1999
  - dc50684819d809fe8fbb3880aeaf10bcac825fa9
  - e10d5cf0ab5fb72cb152b14936b011118e30594a
  - e1b61c2d768c67f81e7bdaee8bd87460320e6cd6
  - e877f3347edaebe931dc8b6d6aa05924ed99a54b
  - f3e6cf7ea23c7c6692f04f83e718721273c21daf
  passport:
    version: 1
    identity: forge-mcp/forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration
    binding:
      version: 1
      repository: forge-mcp
      project: forge-mcp
      milestone: AC-001-hierarchical-acdd-gates-and-milestone-orchestration
      authority_digest: a7886c94550d43924945adb2f3dba6ffc722d3681c9a9b842fdfe41bf9e161cb
      implementation_digest: 7c7999098596b11c357ff089ad5e2c2f7129ed37f372eb5f128a1ce87c833cb0
      target_base_digest: 866dcbfad98548aa4a923ab7e6ec4be0d5c257e6e3b9bca6722b7b63bec6986e
      builders:
      - codex-ac001-builder
      - codex-ac001-synthesis-builder-r4
      - codex-ac001-task1-builder
      - codex-ac001-task2-recipient-builder
      - codex-ac001-task3-builder-20261024
      - codex-ac001-task6-builder-20261025
      - codex-ac001-task7-20261025
      - codex-sol-builder
    commit: e877f3347edaebe931dc8b6d6aa05924ed99a54b
    target_base: 2e63013282c8d97b12b4209841ef34c70e27aa0c
    active_authority_digest: 1e011c5ae7145cc67558cf133dd2c48d2efd7907c58f4ea25bc29a3476302fe9
    started_at: 2026-10-24T02:28:16+00:00
    completed_at: 2026-10-29T13:29:26.115615988+00:00
    duration_seconds: 471670
    tasks_count: 8
    tasks:
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:acdd-test-suite-modularization-and-domain-decomposition
      status: completed
      first_claimed_at: 1792922709
      completed_at: 1792925363
      execution_wall_seconds: 2654
      review_cycles: 1
      rejected_reviews: 0
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:container-gate-and-claim-engine
      status: completed
      first_claimed_at: 1792863739
      completed_at: 1792874209
      execution_wall_seconds: 10470
      review_cycles: 3
      rejected_reviews: 2
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:scope-extension-manifest-sync-and-review-audit
      status: completed
      first_claimed_at: 1792875751
      completed_at: 1792921788
      execution_wall_seconds: 46037
      review_cycles: 12
      rejected_reviews: 8
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:sqlite-gate-transitions-and-trace-persistence
      status: completed
      first_claimed_at: 1792813975
      completed_at: 1792831447
      execution_wall_seconds: 17472
      review_cycles: 2
      rejected_reviews: 0
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:universal-gate-ast-and-modular-catalog-spec
      status: completed
      first_claimed_at: 1792810933
      completed_at: 1792824791
      execution_wall_seconds: 13858
      review_cycles: 6
      rejected_reviews: 5
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:upfront-contract-synthesis-and-dag-prioritization
      status: completed
      first_claimed_at: 1792808896
      completed_at: 1792810871
      execution_wall_seconds: 1975
      review_cycles: 1
      rejected_reviews: 0
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:upfront-contract-synthesis-and-shared-helper-extraction
      status: completed
      first_claimed_at: 1792824966
      completed_at: 1792845966
      execution_wall_seconds: 21000
      review_cycles: 3
      rejected_reviews: 2
    - task_id: forge-mcp/AC-001-hierarchical-acdd-gates-and-milestone-orchestration:zero-arg-transition-engine-and-command-runner
      status: completed
      first_claimed_at: 1792829356
      completed_at: 1792859704
      execution_wall_seconds: 30348
      review_cycles: 4
      rejected_reviews: 3
    diff:
      docs:
      - path: docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
        added: 922
        removed: 0
      - path: docs/adr/README.md
        added: 1
        removed: 0
      - path: docs/future/references/acdd_profile.v2.yaml.example
        added: 355
        removed: 0
      - path: docs/milestones/AC-001-hierarchical-acdd-gates-and-milestone-orchestration.md
        added: 1298
        removed: 0
      - path: docs/milestones/AGENTS.md
        added: 193
        removed: 12
      - path: docs/milestones/README.md
        added: 63
        removed: 0
      - path: docs/reference/acdd.md
        added: 6
        removed: 0
      - path: docs/reference/cli.md
        added: 22
        removed: 6
      - path: docs/reference/configuration.md
        added: 2
        removed: 2
      - path: docs/reference/mcp-tools.md
        added: 24
        removed: 6
      - path: docs/reference/profiles.md
        added: 141
        removed: 62
      - path: docs/reference/tasks.md
        added: 8
        removed: 1
      - path: docs/testing/README.md
        added: 10
        removed: 0
      other:
      - path: .forge/acdd/profile.yaml
        added: 68
        removed: 0
      - path: skills/contextunity-forge/SKILL.md
        added: 14
        removed: 8
      - path: skills/contextunity-forge/references/acdd-profile-examples/README.md
        added: 125
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml
        added: 169
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml
        added: 183
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml
        added: 168
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml
        added: 203
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/milestone-manifest-with-skills.md.example
        added: 92
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml
        added: 160
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml
        added: 397
        removed: 0
      - path: skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml
        added: 149
        removed: 0
      - path: skills/contextunity-forge/references/acdd_profile.current.yaml.example
        added: 0
        removed: 112
      - path: skills/contextunity-forge/references/acdd_profile.yaml.example
        added: 290
        removed: 270
      - path: skills/contextunity-forge/references/agents.md.example
        added: 10
        removed: 8
      src:
      - path: src/cli/guide.rs
        added: 33
        removed: 37
      - path: src/cli/milestone.rs
        added: 135
        removed: 47
      - path: src/cli/mod.rs
        added: 15
        removed: 0
      - path: src/cli/task.rs
        added: 26
        removed: 14
      - path: src/cli/validate.rs
        added: 697
        removed: 0
      - path: src/core/config.rs
        added: 48
        removed: 0
      - path: src/core/git.rs
        added: 100
        removed: 1
      - path: src/core/tasks/acdd.default.yaml
        added: 241
        removed: 95
      - path: src/core/tasks/catalog.rs
        added: 132
        removed: 0
      - path: src/core/tasks/gates.rs
        added: 84
        removed: 2
      - path: src/core/tasks/mod.rs
        added: 375
        removed: 2
      - path: src/core/tasks/profile.rs
        added: 1559
        removed: 48
      - path: src/db/tasks_store/blackboard.rs
        added: 224
        removed: 41
      - path: src/db/tasks_store/lifecycle.rs
        added: 521
        removed: 25
      - path: src/db/tasks_store/matrix.rs
        added: 157
        removed: 0
      - path: src/db/tasks_store/mod.rs
        added: 169
        removed: 26
      - path: src/db/tasks_store/recovery.rs
        added: 303
        removed: 22
      - path: src/db/tasks_store/schema.rs
        added: 61
        removed: 0
      - path: src/db/tasks_store/trace.rs
        added: 570
        removed: 0
      - path: src/engine/milestones/command_gate.rs
        added: 488
        removed: 0
      - path: src/engine/milestones/handoff.rs
        added: 986
        removed: 9
      - path: src/engine/milestones/integration.rs
        added: 320
        removed: 1
      - path: src/engine/milestones/mod.rs
        added: 140
        removed: 5
      - path: src/engine/milestones/synthesis.rs
        added: 688
        removed: 0
      - path: src/engine/tasks/blackboard.rs
        added: 8
        removed: 4
      - path: src/engine/tasks/claim.rs
        added: 326
        removed: 0
      - path: src/engine/tasks/context.rs
        added: 459
        removed: 210
      - path: src/engine/tasks/gates.rs
        added: 820
        removed: 130
      - path: src/engine/tasks/git_snapshot.rs
        added: 591
        removed: 12
      - path: src/engine/tasks/lifecycle.rs
        added: 241
        removed: 17
      - path: src/engine/tasks/mod.rs
        added: 95
        removed: 12
      - path: src/engine/tasks/scheduler.rs
        added: 138
        removed: 0
      - path: src/engine/tasks/submit.rs
        added: 620
        removed: 0
      - path: src/engine/tasks/workspaces.rs
        added: 6
        removed: 1
      - path: src/mcp/response.rs
        added: 1
        removed: 5
      - path: src/mcp/server.rs
        added: 1
        removed: 0
      - path: src/mcp/tools.rs
        added: 98
        removed: 4
      tests:
      - path: tests/AGENTS.md
        added: 6
        removed: 3
      - path: tests/acdd/AGENTS.md
        added: 35
        removed: 0
      - path: tests/acdd/blackboard.rs
        added: 1
        removed: 0
      - path: tests/acdd/contract_synthesis.rs
        added: 207
        removed: 0
      - path: tests/acdd/gates_ast.rs
        added: 864
        removed: 0
      - path: tests/acdd/mcp.rs
        added: 268
        removed: 5
      - path: tests/acdd/milestones.rs
        added: 1103
        removed: 10
      - path: tests/acdd/mod.rs
        added: 7
        removed: 0
      - path: tests/acdd/persistence.rs
        added: 381
        removed: 0
      - path: tests/acdd/runner.rs
        added: 731
        removed: 0
      - path: tests/acdd/scope_sync.rs
        added: 135
        removed: 0
      - path: tests/acdd/seam_reachability.rs
        added: 82
        removed: 0
      - path: tests/acdd/subtasks.rs
        added: 32
        removed: 0
      - path: tests/acdd/support.rs
        added: 243
        removed: 0
      - path: tests/acdd/synthesis.rs
        added: 499
        removed: 0
      - path: tests/acdd/tasks.rs
        added: 0
        removed: 10726
      - path: tests/acdd/tasks/AGENTS.md
        added: 25
        removed: 0
      - path: tests/acdd/tasks/boundaries.rs
        added: 1239
        removed: 0
      - path: tests/acdd/tasks/guidance.rs
        added: 1652
        removed: 0
      - path: tests/acdd/tasks/lifecycle.rs
        added: 3730
        removed: 0
      - path: tests/acdd/tasks/mod.rs
        added: 24
        removed: 0
      - path: tests/acdd/tasks/retirement.rs
        added: 168
        removed: 0
      - path: tests/acdd/tasks/seams.rs
        added: 917
        removed: 0
      - path: tests/acdd/tasks/workspaces.rs
        added: 3568
        removed: 0
      - path: tests/mcp/context.rs
        added: 132
        removed: 81
      - path: tests/mcp/freshness/stdio_and_response.rs
        added: 1
        removed: 0
      - path: tests/mcp/skill_sync.rs
        added: 43
        removed: 7
      - path: tests/mcp/tool_evolution/mcp_boundaries.rs
        added: 41
        removed: 23
    ast_symbol_deltas:
      added:
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.check_full_repo
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.check_full_repo.command
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.check_full_repo.description
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.check_full_repo.timeout_seconds
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.command
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.description
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."src/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."src/main.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/acdd/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/commitment_integrity/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/common/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/core/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/incremental/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/languages/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/linker/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/mcp/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/mcp_context/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/query_context/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.rust-check-and-test.scope_args_by_pattern."tests/receivers/**/*.rs"
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.test
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.test.command
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.commands.test.description
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle.gates
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle.gates.command
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle.gates.contours
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle.gates.id
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle.gates.independent_from
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle.gates.proof
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.gate_templates.build_review_cycle.gates.role
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.deliver
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.deliver.persistence
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.deliver.persistence.post
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.deliver.proof
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.fallback
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.fallback.command
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.fallback.max_attempts
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.fallback.use
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.gates
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.gates.command
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.gates.contours
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.gates.id
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.gates.independent_from
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.gates.proof
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.gates.role
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.milestone_verification.id
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.roles.architect
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.roles.architect.guidance
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.roles.architect.mode
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.roles.architect.models
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.roles.architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.scan_docs_graph_drift
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.scan_docs_graph_drift.command
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.scan_docs_graph_drift.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_full_documentation_suite
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_full_documentation_suite.command
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_full_documentation_suite.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_markdown_links_and_anchors
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_markdown_links_and_anchors.command
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_markdown_links_and_anchors.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_no_code_tampering
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_no_code_tampering.command
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.commands.verify_no_code_tampering.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.receipt
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.review_sources
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.instruction_clarity
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.instruction_clarity.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.instruction_clarity.description
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.link_integrity
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.link_integrity.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.link_integrity.description
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.no_code_tampering
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.no_code_tampering.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.no_code_tampering.description
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.symbol_parity
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.symbol_parity.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.contour.symbol_parity.description
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.docs.drift_scan
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.guidance.docs.reconcile_and_tighten
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_delivery_policy
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_delivery_policy.auto_commit
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_delivery_policy.enforce_clean_worktree
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_review_policy
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_review_policy.audit_extended_scope
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_review_policy.enforce_clickable_file_links
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_review_policy.enforce_no_code_tampering
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_review_policy.independent_worker
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.policies.docs_review_policy.require_all_contours
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_architect
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_architect.mode
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_architect.models
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_builder
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_builder.mode
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_builder.models
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_builder.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_reviewer
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_reviewer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_reviewer.models
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.roles.docs_reviewer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/documentation-graph-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.documentation-graph-refactor.version
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_full_repo
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_full_repo.command
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_full_repo.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_migration_safety
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_migration_safety.command
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_migration_safety.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_task_backend
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_task_backend.command
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.check_task_backend.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.run_playwright_seam
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.run_playwright_seam.command
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.commands.run_playwright_seam.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.contours
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.receipt
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.review_sources
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.gates.tasks.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.commerce.code
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.commerce.contract
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.commerce.e2e
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.claims
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.claims.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.claims.description
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.django_safety
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.django_safety.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.django_safety.description
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.paths
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.paths.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.paths.description
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.query_efficiency
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.query_efficiency.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.guidance.contour.query_efficiency.description
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies.default_delivery
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies.default_delivery.auto_commit
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies.default_delivery.enforce_clean_worktree
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies.default_review
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies.default_review.audit_extended_scope
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies.default_review.independent_worker
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.policies.default_review.require_all_contours
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.architect
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.architect.mode
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.architect.models
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.builder
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.builder.mode
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.builder.models
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.builder.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.e2e_specialist
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.e2e_specialist.mode
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.e2e_specialist.models
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.e2e_specialist.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.reviewer
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.reviewer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.reviewer.models
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.roles.reviewer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/fullstack-commerce-django-playwright.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.fullstack-commerce-django-playwright.version
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.apply_db_migrations
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.apply_db_migrations.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.apply_db_migrations.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.backup_state_checkpoint
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.backup_state_checkpoint.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.backup_state_checkpoint.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.check_infra_syntax
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.check_infra_syntax.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.check_infra_syntax.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.check_migration_dry_run
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.check_migration_dry_run.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.check_migration_dry_run.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.deploy_rolling_containers
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.deploy_rolling_containers.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.deploy_rolling_containers.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.rollback_deployment_and_db
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.rollback_deployment_and_db.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.rollback_deployment_and_db.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.verify_smoke_healthcheck
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.verify_smoke_healthcheck.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.commands.verify_smoke_healthcheck.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.fallback
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.fallback.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.fallback.max_attempts
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.receipt
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.review_sources
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.claims
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.claims.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.claims.description
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.downtime_guards
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.downtime_guards.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.downtime_guards.description
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.infra_safety
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.infra_safety.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.contour.infra_safety.description
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.infra.code
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.guidance.infra.contract
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.policies
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.policies.strict_release
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.policies.strict_release.independent_worker
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.policies.strict_release.require_all_contours
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.policies.strict_release.require_clean_healthcheck
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.infra_architect
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.infra_architect.mode
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.infra_architect.models
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.infra_architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.ops_engineer
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.ops_engineer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.ops_engineer.models
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.ops_engineer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.release_reviewer
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.release_reviewer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.release_reviewer.models
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.roles.release_reviewer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/infrastructure-deploy-database.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.infrastructure-deploy-database.version
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.guard_source_code_untouched
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.guard_source_code_untouched.command
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.guard_source_code_untouched.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.validate_manifest_syntax
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.validate_manifest_syntax.command
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.validate_manifest_syntax.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.verify_dag_acyclicity
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.verify_dag_acyclicity.command
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.verify_dag_acyclicity.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.verify_plan_admission_traceability
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.verify_plan_admission_traceability.command
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.commands.verify_plan_admission_traceability.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.receipt
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.review_sources
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.dag_topology
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.dag_topology.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.dag_topology.description
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.invariant_completeness
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.invariant_completeness.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.invariant_completeness.description
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.no_code_tampering
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.no_code_tampering.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.no_code_tampering.description
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.task_scope_atomicity
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.task_scope_atomicity.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.contour.task_scope_atomicity.description
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.migration.draft_milestones
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.guidance.migration.synthesize_dag
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_delivery_policy
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_delivery_policy.auto_commit
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_delivery_policy.enforce_clean_worktree
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_review_policy
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_review_policy.audit_extended_scope
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_review_policy.enforce_dag_acyclicity
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_review_policy.enforce_schema_validity
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_review_policy.enforce_zero_code_tampering
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_review_policy.independent_worker
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.policies.migration_review_policy.require_all_contours
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.governance_auditor
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.governance_auditor.mode
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.governance_auditor.models
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.governance_auditor.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.milestone_specifier
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.milestone_specifier.mode
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.milestone_specifier.models
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.milestone_specifier.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.portfolio_architect
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.portfolio_architect.mode
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.portfolio_architect.models
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.roles.portfolio_architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/legacy-plans-to-milestones-migration.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.legacy-plans-to-milestones-migration.version
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_all_services
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_all_services.command
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_all_services.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_task
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_task.by_pattern
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_task.by_pattern."backend/**/*.py"
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_task.by_pattern."frontend/**/*.{ts,tsx,vue}"
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_task.by_pattern."infra/**/*.{tf,yaml}"
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.check_task.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.verify_cross_repo_e2e
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.verify_cross_repo_e2e.command
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.commands.verify_cross_repo_e2e.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.contours
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.receipt
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.review_sources
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.gates.tasks.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.cross_repo_compatibility
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.cross_repo_compatibility.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.cross_repo_compatibility.description
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.isolation
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.isolation.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.isolation.description
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.paths
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.paths.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.contour.paths.description
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.service.code
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.guidance.umbrella.synthesis
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.policies
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.policies.umbrella_delivery
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.policies.umbrella_delivery.auto_commit
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.policies.umbrella_delivery.enforce_clean_worktree
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.policies.umbrella_review
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.policies.umbrella_review.independent_worker
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.policies.umbrella_review.require_all_contours
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.integration_reviewer
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.integration_reviewer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.integration_reviewer.models
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.integration_reviewer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.lead_architect
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.lead_architect.mode
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.lead_architect.models
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.lead_architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.service_builder
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.service_builder.mode
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.service_builder.models
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.roles.service_builder.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/multirepo-umbrella-cross-service.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.multirepo-umbrella-cross-service.version
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_docs
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_docs.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_docs.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_full_repo
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_full_repo.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_full_repo.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_task
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_task.by_pattern
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_task.by_pattern."**/*.py"
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_task.by_pattern."**/*.rs"
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_task.by_pattern."**/*.{ts,tsx}"
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.check_task.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.dispatch_seam_webhook
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.dispatch_seam_webhook.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.dispatch_seam_webhook.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.notify_seam_blackboard
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.notify_seam_blackboard.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.commands.notify_seam_blackboard.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.absorb_working_tree
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.contours
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.gates.when
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.persistence.git_commit_note
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.persistence.pre
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.contours
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.expected_failure
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.expected_failure.exit_codes
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.expected_failure.stderr_contains
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.independent_from
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.gates.when
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.independent_from
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.persistence.git_commit_note
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.persistence.pre
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.receipt
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.gates.tasks.gates.when
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.limits
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.limits.max_bytes
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.format
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections.ast_delta
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections.diff_breakdown
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections.duration
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections.review_cycles
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections.tasks_summary
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections.test_suite
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.git_commit_note.telemetry.sections.topology
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.administration
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.administration.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.administration.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.claims
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.claims.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.claims.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.concurrency
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.concurrency.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.concurrency.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.paths
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.paths.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.paths.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.project_isolation
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.project_isolation.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.contour.project_isolation.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.milestone.review
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.milestone.synthesis
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.task.code
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.task.contract
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.task.deferred_final_test
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.task.doc_audit
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.guidance.task.seam_review
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.limits
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.limits.max_metadata_bytes
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.limits.max_trace_summary_bytes
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.policies
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.policies.default_review
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.policies.default_review.audit_extended_scope
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.policies.default_review.independent_worker
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.policies.default_review.require_all_contours
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.exit_code
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.exit_code.const
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.exit_code.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.tests_failed
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.tests_failed.const
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.tests_failed.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.tests_passed
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.properties.tests_passed.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.required
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.command.schema.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.properties
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.properties.contract_test_ref
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.properties.contract_test_ref.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.properties.exit_code
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.properties.exit_code.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.properties.stderr
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.properties.stderr.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.required
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.contract.schema.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema.properties
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema.properties.commit_sha
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema.properties.commit_sha.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema.properties.receipt_id
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema.properties.receipt_id.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema.required
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.delivery.schema.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.contours_reviewed
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.contours_reviewed.items
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.contours_reviewed.items.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.contours_reviewed.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.notes
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.notes.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.reviewer
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.reviewer.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.verdict
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.verdict.enum
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.properties.verdict.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.required
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.review.schema.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.description
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.exit_code
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.exit_code.const
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.exit_code.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.seam_test_refs
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.seam_test_refs.items
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.seam_test_refs.items.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.seam_test_refs.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.tests_failed
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.tests_failed.const
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.tests_failed.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.tests_passed
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.properties.tests_passed.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.required
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.proofs.seam_test.schema.type
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.architect
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.architect.mode
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.architect.models
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.builder
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.builder.mode
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.builder.models
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.builder.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.doc_writer
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.doc_writer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.doc_writer.models
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.doc_writer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.reviewer
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.reviewer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.reviewer.models
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.reviewer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.seam_auditor
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.seam_auditor.mode
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.seam_auditor.models
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.roles.seam_auditor.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/seam-logger-and-event-telemetry.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.seam-logger-and-event-telemetry.version
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.freeze_baseline_inventory
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.freeze_baseline_inventory.command
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.freeze_baseline_inventory.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_full_test_harness
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_full_test_harness.command
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_full_test_harness.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_scoped_refactor
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_scoped_refactor.command
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_scoped_refactor.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_suite_parity_and_linter
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_suite_parity_and_linter.command
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.commands.verify_suite_parity_and_linter.timeout_seconds
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.contours
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.gates
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.gates.command
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.id
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.persistence
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.persistence.immutable
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.persistence.post
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.policy
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.proof
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.receipt
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.reject_to
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.review_sources
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.gates.tasks.gates.role
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.coverage_invariants
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.coverage_invariants.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.coverage_invariants.description
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.paths
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.paths.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.paths.description
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.purity
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.purity.criteria
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.contour.purity.description
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.refactor.consolidate
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.guidance.refactor.inventory
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.policies
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.policies.strict_audit
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.policies.strict_audit.enforce_baseline_parity
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.policies.strict_audit.independent_worker
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.policies.strict_audit.require_all_contours
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.coverage_auditor
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.coverage_auditor.mode
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.coverage_auditor.models
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.coverage_auditor.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.refactorer
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.refactorer.mode
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.refactorer.models
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.refactorer.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.test_architect
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.test_architect.mode
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.test_architect.models
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.roles.test_architect.reuse_on_reject
      - skills/contextunity-forge/references/acdd-profile-examples/test-suite-refactor.yaml:variable:skills.contextunity-forge.references.acdd-profile-examples.test-suite-refactor.version
      - src/cli/guide.rs:function:src.cli.guide.checkpoint_git_tree
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.active
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.depends_on
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.desc
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.dir
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.num
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.plan
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.slug
      - src/cli/milestone.rs:field:src.cli.milestone.DraftArgs.title
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.action
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.draft
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.in_place
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.manifest_path
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.review_file
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.worker_id
      - src/cli/milestone.rs:function:src.cli.milestone.run_draft
      - src/cli/milestone.rs:impl:src.cli.milestone.DraftArgs
      - src/cli/milestone.rs:method:src.cli.milestone.DraftArgs.has_options
      - src/cli/milestone.rs:struct:src.cli.milestone.DraftArgs
      - src/cli/mod.rs:field:src.cli.Command.path
      - src/cli/mod.rs:module_declaration:src.cli.validate
      - src/cli/task.rs:field:src.cli.task.BlackboardCommand.recipient_task_id
      - src/cli/validate.rs:enum:src.cli.validate.DocumentKind
      - src/cli/validate.rs:field:src.cli.validate.Diagnostic.code
      - src/cli/validate.rs:field:src.cli.validate.Diagnostic.column
      - src/cli/validate.rs:field:src.cli.validate.Diagnostic.line
      - src/cli/validate.rs:field:src.cli.validate.Diagnostic.message
      - src/cli/validate.rs:field:src.cli.validate.Diagnostic.path
      - src/cli/validate.rs:field:src.cli.validate.Diagnostic.status
      - src/cli/validate.rs:field:src.cli.validate.ParsedManifest.milestone
      - src/cli/validate.rs:field:src.cli.validate.ParsedManifest.path
      - src/cli/validate.rs:field:src.cli.validate.ParsedManifest.text
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.dependencies
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.line
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.milestone_id
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.path
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.project
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.repository
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.scopes
      - src/cli/validate.rs:field:src.cli.validate.ValidationTask.task_ref
      - src/cli/validate.rs:function:src.cli.validate.active_profile_path
      - src/cli/validate.rs:function:src.cli.validate.configured_dirs_empty
      - src/cli/validate.rs:function:src.cli.validate.error_code
      - src/cli/validate.rs:function:src.cli.validate.fail
      - src/cli/validate.rs:function:src.cli.validate.fail_from_error
      - src/cli/validate.rs:function:src.cli.validate.first_task_line
      - src/cli/validate.rs:function:src.cli.validate.frontmatter_status
      - src/cli/validate.rs:function:src.cli.validate.open_manifest_paths
      - src/cli/validate.rs:function:src.cli.validate.pass
      - src/cli/validate.rs:function:src.cli.validate.path_kind
      - src/cli/validate.rs:function:src.cli.validate.report
      - src/cli/validate.rs:function:src.cli.validate.run
      - src/cli/validate.rs:function:src.cli.validate.scan_open_manifests
      - src/cli/validate.rs:function:src.cli.validate.scan_open_manifests.walk
      - src/cli/validate.rs:function:src.cli.validate.scope_paths_overlap
      - src/cli/validate.rs:function:src.cli.validate.task_line
      - src/cli/validate.rs:function:src.cli.validate.validate_manifest
      - src/cli/validate.rs:function:src.cli.validate.validate_profile
      - src/cli/validate.rs:function:src.cli.validate.validate_task_graph
      - src/cli/validate.rs:struct:src.cli.validate.Diagnostic
      - src/cli/validate.rs:struct:src.cli.validate.ParsedManifest
      - src/cli/validate.rs:struct:src.cli.validate.ValidationTask
      - src/core/config.rs:enum:src.core.config.SubsystemRegistryFile
      - src/core/config.rs:field:src.core.config.Adapter.modules
      - src/core/config.rs:field:src.core.config.AdapterFile.modules
      - src/core/config.rs:field:src.core.config.AdapterFile.profile
      - src/core/config.rs:function:src.core.config.deserialize_subsystem_registry
      - src/core/git.rs:function:src.core.git.git_scoped_commit
      - src/core/git.rs:impl:src.core.git.git_scoped_commit.IndexCleanup
      - src/core/git.rs:method:src.core.git.git_scoped_commit.IndexCleanup.drop
      - src/core/git.rs:struct:src.core.git.git_scoped_commit.IndexCleanup
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.check_full_repo
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.check_full_repo.by_pattern
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.check_full_repo.by_pattern."**/*.py"
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.check_full_repo.by_pattern."**/*.rs"
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.check_full_repo.by_pattern."**/*.{ts,tsx,vue}"
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.check_full_repo.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.check_full_repo.timeout_seconds
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.rust-check-and-test.by_pattern
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.rust-check-and-test.by_pattern."**/*.rs"
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.test.by_pattern
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.test.by_pattern."**/*.rs"
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates.contours
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates.id
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates.independent_from
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates.policy
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates.proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gate_templates.build_review_cycle.gates.role
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.expected_failure
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.expected_failure.exit_codes
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.expected_failure.stderr_contains
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.guidance
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.persistence
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.persistence.git_commit_note
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.persistence.immutable
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.persistence.post
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.persistence.pre
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.policy
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.receipt
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.reject_routing
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.reject_routing.scope
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.reject_routing.symbols
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.format
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.limits
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.limits.max_bytes
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.format
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections.ast_delta
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections.diff_breakdown
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections.duration
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections.review_cycles
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections.tasks_summary
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections.test_suite
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.git_commit_note.telemetry.sections.topology
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.administration
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.administration.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.administration.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.claims
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.claims.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.claims.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.concurrency
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.concurrency.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.concurrency.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.paths
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.paths.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.paths.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.project_isolation
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.project_isolation.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.contour.project_isolation.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.hitl.careless
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.hitl.curious
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.hitl.paranoic
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.hitl.pro
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.milestone.doc
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.milestone.handoff
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.milestone.polish
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.milestone.review
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.milestone.synthesis
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.review.policy
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.review.policy.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.review.policy.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.subtask.dod
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.subtask.dod.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.subtask.dod.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.task.code
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.task.contract
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.task.deferred_final_test
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.task.review
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.guidance.task.seam_review
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.limits
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.limits.max_metadata_bytes
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.limits.max_trace_summary_bytes
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.deliver
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.deliver.persistence
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.deliver.persistence.immutable
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.deliver.persistence.post
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.deliver.proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.deliver.receipt
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.fallback
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.fallback.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.fallback.guidance
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.fallback.max_attempts
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.fallback.use
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.contours
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.guidance
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.id
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.independent_from
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.persistence
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.persistence.immutable
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.policy
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.gates.role
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.milestone_verification.id
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.policies.default_review
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.policies.default_review.audit_extended_scope
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.policies.default_review.independent_worker
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.policies.default_review.require_all_contours
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.command.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.exit_code
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.exit_code.const
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.exit_code.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.tests_failed
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.tests_failed.const
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.tests_failed.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.tests_passed
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.properties.tests_passed.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.properties.command_proof.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.command.schema.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof.properties.red_exit_code
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof.properties.red_exit_code.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof.properties.seam_test_ref
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof.properties.seam_test_ref.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.properties.contract_proof.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.contract.schema.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.commit_sha
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.commit_sha.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.delivered
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.delivered.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.receipt_id
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.receipt_id.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.status
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.properties.status.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.properties.delivery_proof.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.delivery.schema.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.contours
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.contours.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.decision
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.decision.enum
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.decision.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.properties.approved_paths
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.properties.approved_paths.items
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.properties.approved_paths.items.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.properties.approved_paths.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.properties.reason_verified
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.properties.reason_verified.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.extended_scope_review.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.notes
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.properties.notes.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.properties.review_proof.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.review.schema.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.exit_code
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.exit_code.const
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.exit_code.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.seam_test_refs
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.seam_test_refs.items
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.seam_test_refs.items.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.seam_test_refs.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.tests_failed
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.tests_failed.const
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.tests_failed.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.tests_passed
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.properties.tests_passed.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.properties.seam_test_proof.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.required
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.proofs.seam_test.schema.type
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.roles.architect
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.roles.architect.guidance
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.roles.architect.mode
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.roles.architect.models
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.roles.architect.reuse_on_reject
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.version
      - src/core/tasks/catalog.rs:enum:src.core.tasks.catalog.SubsystemPreset
      - src/core/tasks/catalog.rs:field:src.core.tasks.catalog.SubsystemRegistry.disabled
      - src/core/tasks/catalog.rs:field:src.core.tasks.catalog.SubsystemRegistry.enabled
      - src/core/tasks/catalog.rs:field:src.core.tasks.catalog.SubsystemRegistry.profile
      - src/core/tasks/catalog.rs:field:src.core.tasks.catalog.SubsystemRegistry.required
      - src/core/tasks/catalog.rs:function:src.core.tasks.catalog.disabled_tool_error
      - src/core/tasks/catalog.rs:function:src.core.tasks.catalog.filter_subsystem_tools
      - src/core/tasks/catalog.rs:function:src.core.tasks.catalog.is_known_subsystem
      - src/core/tasks/catalog.rs:impl:src.core.tasks.catalog.SubsystemRegistry
      - src/core/tasks/catalog.rs:method:src.core.tasks.catalog.SubsystemRegistry.default
      - src/core/tasks/catalog.rs:method:src.core.tasks.catalog.SubsystemRegistry.is_enabled
      - src/core/tasks/catalog.rs:method:src.core.tasks.catalog.SubsystemRegistry.subsystem_for_tool
      - src/core/tasks/catalog.rs:method:src.core.tasks.catalog.SubsystemRegistry.validate
      - src/core/tasks/catalog.rs:struct:src.core.tasks.catalog.SubsystemRegistry
      - src/core/tasks/gates.rs:field:src.core.tasks.gates.ExtendedScopeReview.approved_paths
      - src/core/tasks/gates.rs:field:src.core.tasks.gates.ExtendedScopeReview.reason_verified
      - src/core/tasks/gates.rs:field:src.core.tasks.gates.ReviewProof.extended_scope_review
      - src/core/tasks/gates.rs:field:src.core.tasks.gates.SeamTestProof.exit_code
      - src/core/tasks/gates.rs:field:src.core.tasks.gates.SeamTestProof.seam_test_refs
      - src/core/tasks/gates.rs:field:src.core.tasks.gates.SeamTestProof.tests_failed
      - src/core/tasks/gates.rs:field:src.core.tasks.gates.SeamTestProof.tests_passed
      - src/core/tasks/gates.rs:function:src.core.tasks.gates.validate_seam_test
      - src/core/tasks/gates.rs:struct:src.core.tasks.gates.ExtendedScopeReview
      - src/core/tasks/gates.rs:struct:src.core.tasks.gates.SeamTestProof
      - src/core/tasks/mod.rs:enum:src.core.tasks.AcddErrorCode
      - src/core/tasks/mod.rs:enum:src.core.tasks.ConcurrencyMode
      - src/core/tasks/mod.rs:enum:src.core.tasks.ConcurrencySync
      - src/core/tasks/mod.rs:enum:src.core.tasks.ContractStatus
      - src/core/tasks/mod.rs:enum:src.core.tasks.ContractType
      - src/core/tasks/mod.rs:enum:src.core.tasks.RepositorySpec
      - src/core/tasks/mod.rs:field:src.core.tasks.AcddError.code
      - src/core/tasks/mod.rs:field:src.core.tasks.AcddError.message
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.milestone_concurrency
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.milestone_guidance
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.milestone_linked_to
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.milestone_repositories
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.milestone_required_skills
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.task_concurrency
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.task_extended_scope
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.task_extended_symbols
      - src/core/tasks/mod.rs:field:src.core.tasks.AuthoritySnapshot.task_symbols
      - src/core/tasks/mod.rs:field:src.core.tasks.ConcurrencyPolicy.mode
      - src/core/tasks/mod.rs:field:src.core.tasks.ConcurrencyPolicy.sync_on
      - src/core/tasks/mod.rs:field:src.core.tasks.Milestone.concurrency
      - src/core/tasks/mod.rs:field:src.core.tasks.Milestone.guidance
      - src/core/tasks/mod.rs:field:src.core.tasks.Milestone.linked_to
      - src/core/tasks/mod.rs:field:src.core.tasks.Milestone.repositories
      - src/core/tasks/mod.rs:field:src.core.tasks.Milestone.required_skills
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractEntry.contract_ref
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractEntry.contract_type
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractEntry.milestone_id
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractEntry.revision
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractEntry.source_task
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractEntry.status
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractEntry.target_task
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractMatrix.entries
      - src/core/tasks/mod.rs:field:src.core.tasks.MilestoneContractMatrix.milestone_id
      - src/core/tasks/mod.rs:field:src.core.tasks.RepositoryDef.name
      - src/core/tasks/mod.rs:field:src.core.tasks.RepositoryDef.path
      - src/core/tasks/mod.rs:field:src.core.tasks.RepositoryDef.project
      - src/core/tasks/mod.rs:field:src.core.tasks.RepositoryDef.repository
      - src/core/tasks/mod.rs:field:src.core.tasks.TaskSpec.concurrency
      - src/core/tasks/mod.rs:field:src.core.tasks.TaskSpec.extended_scope
      - src/core/tasks/mod.rs:field:src.core.tasks.TaskSpec.extended_symbols
      - src/core/tasks/mod.rs:field:src.core.tasks.TaskSpec.symbols
      - src/core/tasks/mod.rs:function:src.core.tasks.validate_concurrency
      - src/core/tasks/mod.rs:function:src.core.tasks.validate_symbol_ref
      - src/core/tasks/mod.rs:impl:src.core.tasks.AcddError
      - src/core/tasks/mod.rs:impl:src.core.tasks.AcddErrorCode
      - src/core/tasks/mod.rs:method:src.core.tasks.AcddError.fmt
      - src/core/tasks/mod.rs:method:src.core.tasks.AcddError.new
      - src/core/tasks/mod.rs:method:src.core.tasks.AcddErrorCode.as_str
      - src/core/tasks/mod.rs:module_declaration:src.core.tasks.catalog
      - src/core/tasks/mod.rs:struct:src.core.tasks.AcddError
      - src/core/tasks/mod.rs:struct:src.core.tasks.ConcurrencyPolicy
      - src/core/tasks/mod.rs:struct:src.core.tasks.MilestoneContractEntry
      - src/core/tasks/mod.rs:struct:src.core.tasks.MilestoneContractMatrix
      - src/core/tasks/mod.rs:struct:src.core.tasks.RepositoryDef
      - src/core/tasks/profile.rs:enum:src.core.tasks.profile.GateDriver
      - src/core/tasks/profile.rs:enum:src.core.tasks.profile.GuidanceEntry
      - src/core/tasks/profile.rs:enum:src.core.tasks.profile.PersistenceAction
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.CommandDef.timeout_seconds
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ExpectedFailure.exit_codes
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ExpectedFailure.stderr_contains
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.absorb_working_tree
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.command
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.deliver
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.expected_failure
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.fallback
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.gates
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.guidance
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.max_attempts
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.persistence
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.policy
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.receipt
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.reject_routing
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.symbols
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.tasks
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.use_template
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateDef.when
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateFallback.command
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateFallback.guidance
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateFallback.max_attempts
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateFallback.use_template
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.gate_templates
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.git_commit_note
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.guidance
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.limits
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.milestone_verification
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.modules
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.proofs
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.scope_args_by_pattern
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.version
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateTemplate.command
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateTemplate.gates
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateTemplate.guidance
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateTemplate.max_attempts
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GitCommitNoteLimits.max_bytes
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GitCommitNotePolicy.format
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GitCommitNotePolicy.limits
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GitCommitNotePolicy.sections
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GitCommitNotePolicy.telemetry
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GitCommitNoteTelemetry.format
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GitCommitNoteTelemetry.sections
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GuidanceEntry.criteria
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GuidanceEntry.description
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ParsedProfile.commands_declared
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ParsedProfile.inherit_defaults
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ParsedProfile.profile
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PatternCommands.by_pattern
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PatternCommands.description
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PatternCommands.timeout_seconds
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PersistencePolicy.git_commit_note
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PersistencePolicy.immutable
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PersistencePolicy.post
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PersistencePolicy.pre
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PolicyDef.description
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.PolicyDef.settings
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ProofCatalogDef.description
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ProofCatalogDef.schema
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.RejectRouting.scope
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.RejectRouting.symbols
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ResourceLimits.max_metadata_bytes
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ResourceLimits.max_trace_summary_bytes
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.SchemaDef.const_value
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.TaskGateBlock.gates
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.GateProfile.executable_gates.collect
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.GateProfile.hierarchical_gate_reference_count.collect
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.GateProfile.validate_guidance_bindings.collect_gate_guidance
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.GateProfile.validate_template_cycles.visit
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.GateProfile.validate_template_cycles.visit.dependencies
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.gate_has_named_id
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.gate_is_hierarchical
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.glob_match_bytes
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.glob_pattern_matches
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.local_gate_reference
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.merge_profile_document
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.normalize_gate
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.normalize_gate_condition
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.normalize_gate_hierarchy
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.parse_profile_document
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.validate_policy_setting
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.validate_profile_document
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.GateDef
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.GitCommitNoteLimits
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.GuidanceEntry
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.PolicyDef
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.ProofKind
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.ResourceLimits
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.CommandDef.command_for_path
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.CommandDef.command_for_root
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.CommandDef.is_valid
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateDef.child_gates
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateDef.driver
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.contract_zero_exit_policies
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.executable_gates
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.hierarchical_gate_reference_count
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.resolve_catalog_key
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.resolve_gate_guidance
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.review_policy_criteria
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.scope_args_for_path
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.subtask_dod_criteria
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_catalogs
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_commands
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_contours
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_gate_fields
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_gate_sequence
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_guidance_bindings
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_guidance_refs
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_hierarchical
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_language_agnostic_defaults
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_milestone_verification
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_ordered_refs
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate_template_cycles
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GitCommitNoteLimits.default
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GuidanceEntry.criteria
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GuidanceEntry.from
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GuidanceEntry.text
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.PolicyDef.validate
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.ProofDef.default
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.ProofKind.as_str
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.ResourceLimits.default
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.ExpectedFailure
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.GateFallback
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.GateTemplate
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.GitCommitNoteLimits
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.GitCommitNotePolicy
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.GitCommitNoteTelemetry
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.ParsedProfile
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.PatternCommands
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.PersistencePolicy
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.PolicyDef
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.ProofCatalogDef
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.RejectRouting
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.ResourceLimits
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.TaskGateBlock
      - src/db/tasks_store/blackboard.rs:function:src.db.tasks_store.blackboard.insert_milestone_broadcast_in_transaction
      - src/db/tasks_store/blackboard.rs:function:src.db.tasks_store.blackboard.validate_milestone_broadcast
      - src/db/tasks_store/blackboard.rs:function:src.db.tasks_store.blackboard.validate_milestone_broadcast_fields
      - src/db/tasks_store/blackboard.rs:method:src.db.tasks_store.blackboard.TasksStore.blackboard_mark_read
      - src/db/tasks_store/blackboard.rs:method:src.db.tasks_store.blackboard.TasksStore.blackboard_page_for_recipient
      - src/db/tasks_store/blackboard.rs:method:src.db.tasks_store.blackboard.TasksStore.blackboard_post_scoped_with_recipient
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.candidate_fingerprint
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.claim_revision
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.contract_revision
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.gate_path
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.milestone_id
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.run_id
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.status
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.subject
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.worker_id
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun.worktree
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionTrace.run
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionTrace.trace
      - src/db/tasks_store/lifecycle.rs:field:src.db.tasks_store.lifecycle.ClaimBoundExecutionTrace.trace_order
      - src/db/tasks_store/lifecycle.rs:function:src.db.tasks_store.lifecycle.execution_run_from_row
      - src/db/tasks_store/lifecycle.rs:function:src.db.tasks_store.lifecycle.execution_run_row
      - src/db/tasks_store/lifecycle.rs:function:src.db.tasks_store.lifecycle.record_transition
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.claim_bound_execution_traces
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.complete_execution_run
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.execution_traces
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.extend_scope_with_pre_commit
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.latest_claim_bound_execution_run
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.record_claim_bound_execution_trace
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.record_gate_transition
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.reserve_execution_attempt
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.start_execution_run
      - src/db/tasks_store/lifecycle.rs:struct:src.db.tasks_store.lifecycle.ClaimBoundExecutionRun
      - src/db/tasks_store/lifecycle.rs:struct:src.db.tasks_store.lifecycle.ClaimBoundExecutionTrace
      - src/db/tasks_store/matrix.rs:function:src.db.tasks_store.matrix.enum_text
      - src/db/tasks_store/matrix.rs:function:src.db.tasks_store.matrix.migrate
      - src/db/tasks_store/matrix.rs:function:src.db.tasks_store.matrix.parse_enum
      - src/db/tasks_store/matrix.rs:function:src.db.tasks_store.matrix.replace_contract_matrix_in_transaction
      - src/db/tasks_store/matrix.rs:function:src.db.tasks_store.matrix.validate_matrix
      - src/db/tasks_store/matrix.rs:impl:src.db.tasks_store.matrix.TasksStore
      - src/db/tasks_store/matrix.rs:method:src.db.tasks_store.matrix.TasksStore.list_contract_matrix
      - src/db/tasks_store/matrix.rs:method:src.db.tasks_store.matrix.TasksStore.replace_contract_matrix
      - src/db/tasks_store/mod.rs:field:src.db.tasks_store.BlackboardMessage.recipient_task_id
      - src/db/tasks_store/mod.rs:function:src.db.tasks_store.ensure_execution_provenance_schema
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.sync_with_contract_matrix_and_initial_message
      - src/db/tasks_store/mod.rs:module_declaration:src.db.tasks_store.matrix
      - src/db/tasks_store/mod.rs:module_declaration:src.db.tasks_store.schema
      - src/db/tasks_store/mod.rs:module_declaration:src.db.tasks_store.trace
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManagedTransaction.database_path
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManagedTransaction.manifest_replacement
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManagedTransaction.publication_fence
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManifestReplacement.before_hash
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManifestReplacement.relative_path
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManifestReplacement.root
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManifestReplacement.target
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.ManifestReplacement.target_hash
      - src/db/tasks_store/recovery.rs:field:src.db.tasks_store.recovery.Publication.manifest_replacement
      - src/db/tasks_store/recovery.rs:function:src.db.tasks_store.recovery.clear_pending
      - src/db/tasks_store/recovery.rs:function:src.db.tasks_store.recovery.tests.pending_manifest_publication_replays_for_non_git_task_store
      - src/db/tasks_store/recovery.rs:impl:src.db.tasks_store.recovery.ManifestReplacement
      - src/db/tasks_store/recovery.rs:impl:src.db.tasks_store.recovery.tests.TestTempDir
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManagedTransaction<'_>.commit_with_publisher
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManagedTransaction<'_>.persistent_database_path
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManagedTransaction<'_>.stage_manifest_replacement
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManifestReplacement.new
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManifestReplacement.publish
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManifestReplacement.validate
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManifestReplacement.validate_preimage
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.Publication.publish_with
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.tests.TestTempDir.drop
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.tests.TestTempDir.new
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.tests.TestTempDir.path
      - src/db/tasks_store/recovery.rs:module_declaration:src.db.tasks_store.recovery.tests
      - src/db/tasks_store/recovery.rs:struct:src.db.tasks_store.recovery.ManifestReplacement
      - src/db/tasks_store/recovery.rs:struct:src.db.tasks_store.recovery.tests.TestTempDir
      - src/db/tasks_store/schema.rs:function:src.db.tasks_store.schema.has_column
      - src/db/tasks_store/schema.rs:function:src.db.tasks_store.schema.migrate
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateCheckpoint.breadcrumb
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateCheckpoint.gate_path
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateCheckpoint.git_tree_sha
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateCheckpoint.milestone_id
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateCheckpoint.state_payload
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateCheckpoint.subject
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateCheckpoint.transition_id
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.attempt_id
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.command_id
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.command_str
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.duration_ms
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.exit_code
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.gate_path
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.log_path
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.milestone_id
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.output_snippet
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.status
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.stdout_sha256
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateExecutionTrace.subject
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.action
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.gate_path
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.git_tree_sha
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.milestone_id
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.proof_kind
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.proof_payload
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.subject
      - src/db/tasks_store/trace.rs:field:src.db.tasks_store.trace.GateTransition.worker_id
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.bounded_output_snippet
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.checkpoint_from_row
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.checkpoint_identity
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.checkpoint_payload_from_ref
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.is_engine_owned_checkpoint
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.migrate
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.prune_milestone_gate_records
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.record_trace
      - src/db/tasks_store/trace.rs:function:src.db.tasks_store.trace.transition_from_row
      - src/db/tasks_store/trace.rs:impl:src.db.tasks_store.trace.TasksStore
      - src/db/tasks_store/trace.rs:method:src.db.tasks_store.trace.TasksStore.delete_checkpoint
      - src/db/tasks_store/trace.rs:method:src.db.tasks_store.trace.TasksStore.get_checkpoint
      - src/db/tasks_store/trace.rs:method:src.db.tasks_store.trace.TasksStore.get_gate_transition
      - src/db/tasks_store/trace.rs:method:src.db.tasks_store.trace.TasksStore.list_checkpoints
      - src/db/tasks_store/trace.rs:method:src.db.tasks_store.trace.TasksStore.record_execution_trace
      - src/db/tasks_store/trace.rs:method:src.db.tasks_store.trace.TasksStore.save_checkpoint
      - src/db/tasks_store/trace.rs:method:src.db.tasks_store.trace.TasksStore.save_checkpoint_payload
      - src/db/tasks_store/trace.rs:struct:src.db.tasks_store.trace.GateCheckpoint
      - src/db/tasks_store/trace.rs:struct:src.db.tasks_store.trace.GateExecutionTrace
      - src/db/tasks_store/trace.rs:struct:src.db.tasks_store.trace.GateTransition
      - src/engine/milestones/command_gate.rs:field:src.engine.milestones.command_gate.ShellResult.duration_ms
      - src/engine/milestones/command_gate.rs:field:src.engine.milestones.command_gate.ShellResult.exit_code
      - src/engine/milestones/command_gate.rs:field:src.engine.milestones.command_gate.ShellResult.stderr
      - src/engine/milestones/command_gate.rs:field:src.engine.milestones.command_gate.ShellResult.stdout
      - src/engine/milestones/command_gate.rs:field:src.engine.milestones.command_gate.ShellResult.timed_out
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.command_plan
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.create_confined_log
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.digest_key
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.execute_command_gate
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.persist_log
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.read_all
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.run_shell
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.shell_command
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.terminate_process_group
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.terminate_process_tree
      - src/engine/milestones/command_gate.rs:function:src.engine.milestones.command_gate.validate_project_path
      - src/engine/milestones/command_gate.rs:struct:src.engine.milestones.command_gate.ShellResult
      - src/engine/milestones/handoff.rs:enum:src.engine.milestones.handoff.MilestoneVerificationPhase
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.FallbackBuildClaim.base_binding
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.FallbackBuildClaim.base_head
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.FallbackBuildClaim.worker_id
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.binding
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.candidate_head
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.candidate_tree_sha
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.delivery_commit
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.fallback_attempts
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.fallback_build_claim
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.fallback_builder
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.last_failure
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.phase
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.receipt_id
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.review
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.target_base
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.verification
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.MilestoneVerificationState.verification_runs
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.PreparedHandoff.binding
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.PreparedHandoff.commit
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.PreparedHandoff.item
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.PreparedHandoff.store
      - src/engine/milestones/handoff.rs:field:src.engine.milestones.handoff.PreparedHandoff.target
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.action_gate
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.current_tree
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.delivered_verification_matches_current
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.execute_milestone_verification
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.fallback_configuration
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.finish_milestone_verification_delivery
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.is_documentation_path
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.load_milestone_verification_state
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_action_matches_phase
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_breadcrumb
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_build
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_build_claim
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_build_submit
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_identity
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_profile
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.milestone_verification_result
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.prepare_handoff_context
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.require_current_verification_candidate
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.require_fallback_descendant
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.route_milestone_verification_fallback
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.same_fallback_authority
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.save_milestone_verification_state
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.submit_milestone_verification_review
      - src/engine/milestones/handoff.rs:struct:src.engine.milestones.handoff.FallbackBuildClaim
      - src/engine/milestones/handoff.rs:struct:src.engine.milestones.handoff.MilestoneVerificationState
      - src/engine/milestones/handoff.rs:struct:src.engine.milestones.handoff.PreparedHandoff
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.MilestoneVerificationReview.binding
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.MilestoneVerificationReview.review
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.MilestoneVerificationReview.target_base
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.ShellOutput.duration_ms
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.ShellOutput.status
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.ShellOutput.stderr
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.ShellOutput.stdout
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.ShellOutput.timed_out
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.command
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.command_id
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.duration_ms
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.exit_code
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.status
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.stderr_sha256
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.stdout_sha256
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.tests_failed
      - src/engine/milestones/integration.rs:field:src.engine.milestones.integration.VerificationCheck.tests_passed
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.bounded_output
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.read_output
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.receive_output
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.run_milestone_check
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.run_shell
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.shell_command
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.terminate_process_group
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.terminate_process_tree
      - src/engine/milestones/integration.rs:function:src.engine.milestones.integration.test_counts
      - src/engine/milestones/integration.rs:impl:src.engine.milestones.integration.VerificationCheck
      - src/engine/milestones/integration.rs:method:src.engine.milestones.integration.VerificationCheck.passed
      - src/engine/milestones/integration.rs:struct:src.engine.milestones.integration.MilestoneVerificationReview
      - src/engine/milestones/integration.rs:struct:src.engine.milestones.integration.ShellOutput
      - src/engine/milestones/integration.rs:struct:src.engine.milestones.integration.VerificationCheck
      - src/engine/milestones/mod.rs:function:src.engine.milestones.draft
      - src/engine/milestones/mod.rs:function:src.engine.milestones.execute_command_gate
      - src/engine/milestones/mod.rs:function:src.engine.milestones.extract_shared_helpers
      - src/engine/milestones/mod.rs:function:src.engine.milestones.init_from_manifest
      - src/engine/milestones/mod.rs:function:src.engine.milestones.synthesize_milestone_contracts
      - src/engine/milestones/mod.rs:function:src.engine.milestones.validate_wave_dag
      - src/engine/milestones/mod.rs:function:src.engine.milestones.verify_contract_matrix_parity
      - src/engine/milestones/mod.rs:module_declaration:src.engine.milestones.command_gate
      - src/engine/milestones/mod.rs:module_declaration:src.engine.milestones.synthesis
      - src/engine/milestones/synthesis.rs:field:src.engine.milestones.synthesis.Participant.branch
      - src/engine/milestones/synthesis.rs:field:src.engine.milestones.synthesis.Participant.exists
      - src/engine/milestones/synthesis.rs:field:src.engine.milestones.synthesis.Participant.git_root
      - src/engine/milestones/synthesis.rs:field:src.engine.milestones.synthesis.Participant.name
      - src/engine/milestones/synthesis.rs:field:src.engine.milestones.synthesis.Participant.worktree
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.create_worktree
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.extract_shared_helpers
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.git_output
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.git_output_status
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.load_adapter_file
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.manifest_frontmatter
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.normalize_existing_or_target
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.paths_overlap
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.preflight_milestone_init
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.preflight_milestone_init_inner
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.preflight_participants
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.resolve_participants
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.rollback_worktrees
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.shared_contract_type
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.synthesize_milestone_contracts
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.task_paths
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.validate_gate_module_requirements
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.validate_profile_module_requirements
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.validate_required_skills
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.validate_sqlite_revision_ranges
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.validate_wave_dag
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.validate_wave_scopes
      - src/engine/milestones/synthesis.rs:function:src.engine.milestones.synthesis.worktree_branch
      - src/engine/milestones/synthesis.rs:struct:src.engine.milestones.synthesis.Participant
      - src/engine/tasks/blackboard.rs:field:src.engine.tasks.blackboard.BlackboardRequest.recipient_task_id
      - src/engine/tasks/claim.rs:field:src.engine.tasks.claim.GateCursor.canonical_stage_id
      - src/engine/tasks/claim.rs:field:src.engine.tasks.claim.GateCursor.flat_index
      - src/engine/tasks/claim.rs:field:src.engine.tasks.claim.GateCursor.gate
      - src/engine/tasks/claim.rs:field:src.engine.tasks.claim.GateCursor.gate_path
      - src/engine/tasks/claim.rs:field:src.engine.tasks.claim.GateCursor.root_index
      - src/engine/tasks/claim.rs:field:src.engine.tasks.claim.GateCursor.stage_id
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.advance_gate_cursor
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.breadcrumb
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.claim_nested_gate
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.collect_cursors
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.collect_cursors.collect_sequence
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.cursor_for_path
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.cursor_for_path.collect_containers
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.cursor_for_stage
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.flattened_profile
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.gate_sequence
      - src/engine/tasks/claim.rs:function:src.engine.tasks.claim.resolve_subagent_role
      - src/engine/tasks/claim.rs:struct:src.engine.tasks.claim.GateCursor
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.adapt_workspace_guidance
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.is_rfc_message
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.linked_milestone_context
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.prune_blackboard_guidance
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.validate_claim_gate_requirements
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.validate_gate_requirements
      - src/engine/tasks/gates.rs:field:src.engine.tasks.gates.IndexedSeamEdge.evidence
      - src/engine/tasks/gates.rs:field:src.engine.tasks.gates.IndexedSeamEdge.line
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.accepted_evidence_for_cursor
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.accepted_submission_for_cursor
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.call_binding_names
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.current_import_call_path
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.current_public_callable
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.imported_symbol_name
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.indexed_module_import_edge
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.indexed_path_to_scope
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.indexed_seam_node
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.module_identity
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.module_import_references
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.seam_node_label
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.seam_path_label
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.unrestricted_public_use
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.validate_extended_scope_review
      - src/engine/tasks/git_snapshot.rs:field:src.engine.tasks.git_snapshot.NamedSnapshotContext.namespace
      - src/engine/tasks/git_snapshot.rs:field:src.engine.tasks.git_snapshot.NamedSnapshotContext.reference
      - src/engine/tasks/git_snapshot.rs:field:src.engine.tasks.git_snapshot.NamedSnapshotContext.worktree
      - src/engine/tasks/git_snapshot.rs:field:src.engine.tasks.git_snapshot.NamedSnapshotLock._file
      - src/engine/tasks/git_snapshot.rs:field:src.engine.tasks.git_snapshot.NamedSnapshotLock.path
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.add_unscoped_snapshot_entries
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.capture_named_snapshot
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.capture_scoped_snapshot_excluding
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.clean_exclude_pattern
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.create_named_snapshot
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.ensure_named_snapshot_state
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.ensure_snapshot_exclusions_untracked
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.lock_named_snapshot_operation
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.named_snapshot_context
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.named_snapshot_exclusions
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.named_snapshot_ref_exists
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.restore_named_snapshot_tree
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.rollback_named_snapshot
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.snapshot_tree
      - src/engine/tasks/git_snapshot.rs:impl:src.engine.tasks.git_snapshot.NamedSnapshotLock
      - src/engine/tasks/git_snapshot.rs:method:src.engine.tasks.git_snapshot.NamedSnapshotLock.drop
      - src/engine/tasks/git_snapshot.rs:struct:src.engine.tasks.git_snapshot.NamedSnapshotContext
      - src/engine/tasks/git_snapshot.rs:struct:src.engine.tasks.git_snapshot.NamedSnapshotLock
      - src/engine/tasks/lifecycle.rs:function:src.engine.tasks.lifecycle.render_scope_extension_manifest
      - src/engine/tasks/mod.rs:field:src.engine.tasks.Manage.name
      - src/engine/tasks/mod.rs:function:src.engine.tasks.candidate_fingerprint
      - src/engine/tasks/mod.rs:function:src.engine.tasks.compile_evidence_envelope
      - src/engine/tasks/mod.rs:function:src.engine.tasks.evaluate_reject_routing
      - src/engine/tasks/mod.rs:function:src.engine.tasks.rebase_speculative_dependency
      - src/engine/tasks/mod.rs:function:src.engine.tasks.submit_zero_arg
      - src/engine/tasks/mod.rs:function:src.engine.tasks.telemetry_passport_note
      - src/engine/tasks/mod.rs:function:src.engine.tasks.verify_red_to_green
      - src/engine/tasks/mod.rs:module_declaration:src.engine.tasks.claim
      - src/engine/tasks/mod.rs:module_declaration:src.engine.tasks.scheduler
      - src/engine/tasks/mod.rs:module_declaration:src.engine.tasks.submit
      - src/engine/tasks/scheduler.rs:function:src.engine.tasks.scheduler.rebase_speculative_dependency
      - src/engine/tasks/scheduler.rs:function:src.engine.tasks.scheduler.restore_checkout
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.bounded_string
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.candidate_fingerprint
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.command_proof
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.compile_evidence_envelope
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.contract_proof
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.evaluate_reject_routing
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.git_relative_path
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.parse_test_counts
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.read_trace_log
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.seam_reference_from_command
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.submit_zero_arg
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.telemetry_passport_note
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.truncate_utf8
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.validate_trace_path
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.verify_command_identity
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.verify_red_to_green
      - src/engine/tasks/submit.rs:function:src.engine.tasks.submit.verify_registered_command
      - src/mcp/tools.rs:method:src.mcp.tools.Server.list_tools
      - tests/acdd/contract_synthesis.rs:function:tests.acdd.contract_synthesis.missing_dependency_relationship_is_rejected_with_a_stable_diagnostic
      - tests/acdd/gates_ast.rs:function:tests.acdd.gates_ast.linked_milestone_context_reports_upstream_task_status_and_blockers
      - tests/acdd/gates_ast.rs:function:tests.acdd.gates_ast.nested_container_gates_claim_and_execute_cleanly
      - tests/acdd/gates_ast.rs:function:tests.acdd.gates_ast.profile_parser_accepts_polymorphic_drivers_and_catalogs
      - tests/acdd/gates_ast.rs:function:tests.acdd.gates_ast.task_manage_named_snapshot_restores_worktree_and_guards_head_changes
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.commit_verification_candidate
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.commit_verification_documentation
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.commit_verification_skill_script
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_handoff_prunes_only_its_gate_telemetry_identity
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_actions
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_binds_check_review_and_delivery_in_order
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_build
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_fallback_repairs_and_stops_after_two_attempts
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_fixture
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_review
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_review_decision
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_review_rejection_runs_bounded_build_verify_review_cycles
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_verification_trace
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.submit_milestone_verification_review
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.submit_milestone_verification_review_decision
      - tests/acdd/mod.rs:module_declaration:tests.acdd.contract_synthesis
      - tests/acdd/mod.rs:module_declaration:tests.acdd.gates_ast
      - tests/acdd/mod.rs:module_declaration:tests.acdd.persistence
      - tests/acdd/mod.rs:module_declaration:tests.acdd.runner
      - tests/acdd/mod.rs:module_declaration:tests.acdd.scope_sync
      - tests/acdd/mod.rs:module_declaration:tests.acdd.seam_reachability
      - tests/acdd/mod.rs:module_declaration:tests.acdd.synthesis
      - tests/acdd/persistence.rs:function:tests.acdd.persistence.primary_key_columns
      - tests/acdd/persistence.rs:function:tests.acdd.persistence.task_store_migrates_the_ac001_gate_persistence_contract
      - tests/acdd/runner.rs:function:tests.acdd.runner.digest
      - tests/acdd/runner.rs:function:tests.acdd.runner.latest_run
      - tests/acdd/runner.rs:function:tests.acdd.runner.shell_quote
      - tests/acdd/runner.rs:function:tests.acdd.runner.transition_runner_compiles_proofs_and_covers_all_task3_subtasks
      - tests/acdd/scope_sync.rs:function:tests.acdd.scope_sync.extend_scope
      - tests/acdd/scope_sync.rs:function:tests.acdd.scope_sync.scope_extension_keeps_both_projections_unchanged_when_sqlite_writer_is_blocked
      - tests/acdd/scope_sync.rs:function:tests.acdd.scope_sync.scope_extension_rejects_invalid_manifest_without_persisting_sqlite_state
      - tests/acdd/scope_sync.rs:function:tests.acdd.scope_sync.scope_extension_updates_sqlite_and_milestone_manifest
      - tests/acdd/scope_sync.rs:function:tests.acdd.scope_sync.scope_fixture
      - tests/acdd/scope_sync.rs:function:tests.acdd.scope_sync.stored_task
      - tests/acdd/seam_reachability.rs:function:tests.acdd.seam_reachability.contract_seam_follows_public_reexports_from_test_helpers
      - tests/acdd/support.rs:function:tests.acdd.support.add_boundary_edge_fixtures
      - tests/acdd/support.rs:function:tests.acdd.support.commit_fixture_baseline
      - tests/acdd/support.rs:function:tests.acdd.support.function_caller_edge_count
      - tests/acdd/support.rs:function:tests.acdd.support.git
      - tests/acdd/support.rs:function:tests.acdd.support.indexed_boundary_workspace
      - tests/acdd/support.rs:function:tests.acdd.support.indexed_test_path_helper_workspace
      - tests/acdd/support.rs:function:tests.acdd.support.method_caller_edge_count
      - tests/acdd/support.rs:function:tests.acdd.support.prepare_indexed_boundary_workspace
      - tests/acdd/support.rs:function:tests.acdd.support.submit_indexed_boundary
      - tests/acdd/synthesis.rs:field:tests.acdd.synthesis.milestone_init_preflight_checks_skills_before_persisting_task_state.MilestoneCli.command
      - tests/acdd/synthesis.rs:field:tests.acdd.synthesis.milestone_init_preflight_checks_skills_before_persisting_task_state.TaskCli.command
      - tests/acdd/synthesis.rs:function:tests.acdd.synthesis.milestone_init_preflight_checks_skills_before_persisting_task_state
      - tests/acdd/synthesis.rs:function:tests.acdd.synthesis.wave_validation_orders_dependencies_and_rejects_parallel_scope_overlap
      - tests/acdd/synthesis.rs:struct:tests.acdd.synthesis.milestone_init_preflight_checks_skills_before_persisting_task_state.MilestoneCli
      - tests/acdd/synthesis.rs:struct:tests.acdd.synthesis.milestone_init_preflight_checks_skills_before_persisting_task_state.TaskCli
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.boundary_guards_follow_configured_proof_kinds_and_non_cargo_commands_keep_paths
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.commit_scope_validation_checks_exact_milestone_and_non_empty_scope
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.signature_boundary_covers_exported_javascript_callables
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.signature_boundary_covers_exported_typescript_callables
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.signature_boundary_covers_public_python_callables
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.signature_boundary_keeps_contract_acceptance_callers_after_symbol_rename_and_reindex
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.signature_boundary_protects_implicitly_public_trait_methods
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.signature_boundary_uses_exact_method_baseline_across_body_edits_and_reindex
      - tests/acdd/tasks/boundaries.rs:function:tests.acdd.tasks.boundaries.validate_commit_for_task_enforces_scope_and_ancestry_invariants
      - tests/acdd/tasks/guidance.rs:function:tests.acdd.tasks.guidance.claim_context_exposes_recursive_scheme_schema_from_custom_profile
      - tests/acdd/tasks/guidance.rs:function:tests.acdd.tasks.guidance.claim_returns_bundle_by_default_with_task_and_milestone_blackboard_messages
      - tests/acdd/tasks/guidance.rs:function:tests.acdd.tasks.guidance.declarative_profile_compiles_defaults_and_respects_role_overrides
      - tests/acdd/tasks/guidance.rs:function:tests.acdd.tasks.guidance.inline_role_mode_adapts_guidance_text
      - tests/acdd/tasks/guidance.rs:function:tests.acdd.tasks.guidance.task_blackboard_gate_targeting_and_schema_hint
      - tests/acdd/tasks/guidance.rs:function:tests.acdd.tasks.guidance.task_guidance_follows_agent_metadata_workspace_config_and_active_stage
      - tests/acdd/tasks/guidance.rs:function:tests.acdd.tasks.guidance.task_guidance_requires_named_and_installed_forge_skill
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.authority_gap_attributes_milestone_invariants_and_owners_with_specific_repair
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.claim_distinguishes_missing_milestone_file_from_missing_task
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.claim_reports_structured_authority_gap_for_frontmatter_and_task_drift
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.completed_task_reopen_and_mcp_manage_action_lifecycle
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.concurrent_pre_snapshot_immutable_gate_rejects_on_demand_checkpoint_races
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.contract_readmission_and_reset_fence_old_evidence
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.cross_milestone_dependency_diagnostic_gates_readiness_and_claim
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.dedicated_store_rejects_index_and_unknown_schema_without_ddl
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.delivery_rejects_commit_that_differs_from_candidate_when_auto_commit_is_disabled
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.delivery_requires_terminal_subtasks_with_verification_evidence
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.force_deletion_preserves_project_boundaries_and_revision_fencing
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.gates_write_receipts_and_preserve_retention_outcomes
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.milestone_id_change_is_rejected_before_sync_admission_and_claim_lookup
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.milestone_id_guard_ignores_foreign_project_at_the_same_manifest_path
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.on_demand_checkpoint_cannot_override_immutable_candidate_or_claim_checkpoint
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.rejected_task_gates_require_typed_proof_and_persist_failing_evidence
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.review_rejection_retains_findings_and_requires_a_new_build
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.specifications_validate_unique_identities_and_governing_digests
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.sync_reports_structured_authority_gap_for_task_drift_at_equal_revision
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_cli_submit_accepts_json_object_and_persists_gate_evidence
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_list_filters_milestone_status_and_controls_subtask_details
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_manage_selector_diagnostics_report_fields_before_resolution
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_manage_sync_and_list_resolve_milestone_reference_by_id_prefix_stem_and_path
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_mutations_bound_payloads_and_initialized_readers_avoid_writer_locks
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_specification_and_store_survive_code_index_rebuild
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_store_diagnostics_cover_not_found_stale_claim_and_submission_precedence
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.tasks_coordinate_claims_dependencies_reset_scope_and_claim_diagnostic
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.typed_task_proofs_pass_through_engine_and_real_store
      - tests/acdd/tasks/lifecycle.rs:function:tests.acdd.tasks.lifecycle.unsynced_higher_revision_with_header_drift_reports_header_and_sync_action
      - tests/acdd/tasks/mod.rs:module_declaration:tests.acdd.tasks.boundaries
      - tests/acdd/tasks/mod.rs:module_declaration:tests.acdd.tasks.guidance
      - tests/acdd/tasks/mod.rs:module_declaration:tests.acdd.tasks.lifecycle
      - tests/acdd/tasks/mod.rs:module_declaration:tests.acdd.tasks.retirement
      - tests/acdd/tasks/mod.rs:module_declaration:tests.acdd.tasks.seams
      - tests/acdd/tasks/mod.rs:module_declaration:tests.acdd.tasks.workspaces
      - tests/acdd/tasks/retirement.rs:function:tests.acdd.tasks.retirement.build_gate_handles_file_retirement_and_preserves_caller_safety
      - tests/acdd/tasks/retirement.rs:function:tests.acdd.tasks.retirement.build_revalidates_legacy_empty_baseline_caller_evidence
      - tests/acdd/tasks/retirement.rs:function:tests.acdd.tasks.retirement.contract_rejects_retirement_without_baseline_caller_evidence
      - tests/acdd/tasks/seams.rs:function:tests.acdd.tasks.seams.contract_seam_accepts_root_cargo_package_imports_from_integration_tests
      - tests/acdd/tasks/seams.rs:function:tests.acdd.tasks.seams.contract_seam_accepts_test_path_helpers_and_rejects_stale_hops
      - tests/acdd/tasks/seams.rs:function:tests.acdd.tasks.seams.contract_seam_requires_current_indexed_paths_at_every_hop
      - tests/acdd/tasks/seams.rs:function:tests.acdd.tasks.seams.contract_seam_validates_claimed_worktree_against_shared_primary_index
      - tests/acdd/tasks/seams.rs:function:tests.acdd.tasks.seams.greenfield_seam_diagnostic_reports_missing_scope_files_and_scaffolding_guidance
      - tests/acdd/tasks/seams.rs:function:tests.acdd.tasks.seams.seam_scope_directory_prefix_treats_sql_wildcards_as_literal
      - tests/acdd/tasks/seams.rs:function:tests.acdd.tasks.seams.tiered_task_context_enforces_indexed_seams_and_signature_boundaries
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.infer_project_from_path_supports_arbitrary_monorepo_structures_universally
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.linked_pinned_tasks_ignore_linked_workspace_profile_overrides
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.linked_scope_extension_validates_owner_and_claimed_worktree
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.linked_task_workspaces_share_storage_and_confine_local_scope
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.linked_worktree_resolves_tasks_db_to_primary_worktree_root
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.nested_repository_tasks_preserve_distinct_write_perimeters
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.scope_extension_rejects_paths_owned_by_sibling_tasks_and_admits_unowned_tests_and_src
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.scoped_commit_commits_staged_deletion_and_preserves_unrelated_staged_work
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.scoped_snapshot_omits_deleted_exact_path_during_contract_submit
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.task_configuration_defaults_and_crlf_preserve_operational_identity
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.task_profile_resolution_rejects_file_and_parent_symlink_escapes
      - tests/acdd/tasks/workspaces.rs:function:tests.acdd.tasks.workspaces.universal_scope_roots_extend_scope_and_boundary_validation
      - tests/mcp/context.rs:function:tests.mcp.context.checkpoint_breadcrumb
      - tests/mcp/context.rs:function:tests.mcp.context.checkpoint_payload
      - tests/mcp/context.rs:function:tests.mcp.context.checkpoint_snapshot
      - tests/mcp/skill_sync.rs:function:tests.mcp.skill_sync.is_installed_skill_markdown
      removed:
      - src/cli/guide.rs:field:src.cli.guide.Checkpoints.entries
      - src/cli/guide.rs:struct:src.cli.guide.Checkpoints
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.active
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.depends_on
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.desc
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.dir
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.num
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.plan
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.slug
      - src/cli/milestone.rs:field:src.cli.milestone.MilestoneCommand.title
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.rust-check-and-test.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.test.command
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.completed
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.completed.steps
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.completed.tools
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.administration
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.administration.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.administration.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.claims
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.claims.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.claims.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.concurrency
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.concurrency.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.concurrency.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.paths
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.paths.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.paths.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.project_isolation
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.project_isolation.criteria
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contours.standard.project_isolation.description
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.contract_zero_exit_policies
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.auto_commit
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.receipt_review
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.sha_snapshot
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.steps
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.tools
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.review_policy
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.subtask_dod
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.contours
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.review_policy
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.subtask_dod
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.cargo_test_target
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.accepted
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.add_boundary_edge_fixtures
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.authority_gap_attributes_milestone_invariants_and_owners_with_specific_repair
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.boundary_guards_follow_configured_proof_kinds_and_non_cargo_commands_keep_paths
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.build_gate_handles_file_retirement_and_preserves_caller_safety
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.build_revalidates_legacy_empty_baseline_caller_evidence
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.claim_context_exposes_recursive_scheme_schema_from_custom_profile
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.claim_distinguishes_missing_milestone_file_from_missing_task
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.claim_reports_structured_authority_gap_for_frontmatter_and_task_drift
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.claim_returns_bundle_by_default_with_task_and_milestone_blackboard_messages
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.commit_fixture_baseline
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.commit_scope_validation_checks_exact_milestone_and_non_empty_scope
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.completed_task_reopen_and_mcp_manage_action_lifecycle
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.contract_readmission_and_reset_fence_old_evidence
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.contract_rejects_retirement_without_baseline_caller_evidence
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.contract_seam_accepts_root_cargo_package_imports_from_integration_tests
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.contract_seam_accepts_test_path_helpers_and_rejects_stale_hops
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.contract_seam_requires_current_indexed_paths_at_every_hop
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.contract_seam_validates_claimed_worktree_against_shared_primary_index
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.cross_milestone_dependency_diagnostic_gates_readiness_and_claim
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.declarative_profile_compiles_defaults_and_respects_role_overrides
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.dedicated_store_rejects_index_and_unknown_schema_without_ddl
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.delivery_rejects_commit_that_differs_from_candidate_when_auto_commit_is_disabled
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.force_deletion_preserves_project_boundaries_and_revision_fencing
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.function_caller_edge_count
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.gates_write_receipts_and_preserve_retention_outcomes
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.greenfield_seam_diagnostic_reports_missing_scope_files_and_scaffolding_guidance
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.indexed_boundary_workspace
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.indexed_test_path_helper_workspace
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.infer_project_from_path_supports_arbitrary_monorepo_structures_universally
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.inline_role_mode_adapts_guidance_text
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.linked_pinned_tasks_ignore_linked_workspace_profile_overrides
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.linked_scope_extension_validates_owner_and_claimed_worktree
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.linked_task_workspaces_share_storage_and_confine_local_scope
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.linked_worktree_resolves_tasks_db_to_primary_worktree_root
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.method_caller_edge_count
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.milestone_id_change_is_rejected_before_sync_admission_and_claim_lookup
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.milestone_id_guard_ignores_foreign_project_at_the_same_manifest_path
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.nested_repository_tasks_preserve_distinct_write_perimeters
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.prepare_indexed_boundary_workspace
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.rejected_task_gates_require_typed_proof_and_persist_failing_evidence
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.review_rejection_retains_findings_and_requires_a_new_build
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.scope_extension_rejects_paths_owned_by_sibling_tasks_and_admits_unowned_tests_and_src
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.scoped_git_snapshot_captures_untracked_and_modified_files_and_cleans_up_on_handoff
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.scoped_snapshot_omits_deleted_exact_path_during_contract_submit
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.seam_scope_directory_prefix_treats_sql_wildcards_as_literal
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.signature_boundary_covers_exported_javascript_callables
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.signature_boundary_covers_exported_typescript_callables
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.signature_boundary_covers_public_python_callables
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.signature_boundary_keeps_contract_acceptance_callers_after_symbol_rename_and_reindex
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.signature_boundary_protects_implicitly_public_trait_methods
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.signature_boundary_uses_exact_method_baseline_across_body_edits_and_reindex
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.specifications_validate_unique_identities_and_governing_digests
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.submit_indexed_boundary
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.sync_reports_structured_authority_gap_for_task_drift_at_equal_revision
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_blackboard_gate_targeting_and_schema_hint
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_cli_submit_accepts_json_object_and_persists_gate_evidence
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_configuration_defaults_and_crlf_preserve_operational_identity
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_guidance_follows_agent_metadata_workspace_config_and_active_stage
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_guidance_requires_named_and_installed_forge_skill
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_list_filters_milestone_status_and_controls_subtask_details
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_manage_selector_diagnostics_report_fields_before_resolution
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_manage_sync_and_list_resolve_milestone_reference_by_id_prefix_stem_and_path
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_mutations_bound_payloads_and_initialized_readers_avoid_writer_locks
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_profile_resolution_rejects_file_and_parent_symlink_escapes
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_specification_and_store_survive_code_index_rebuild
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_store_diagnostics_cover_not_found_stale_claim_and_submission_precedence
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.task_submit_accepts_inline_json_evidence_and_persists_it_in_sqlite
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.tasks_coordinate_claims_dependencies_reset_scope_and_claim_diagnostic
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.tiered_task_context_enforces_indexed_seams_and_signature_boundaries
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.typed_task_proofs_pass_through_engine_and_real_store
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.universal_scope_roots_extend_scope_and_boundary_validation
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.unsynced_higher_revision_with_header_drift_reports_header_and_sync_action
      - tests/acdd/tasks.rs:function:tests.acdd.tasks.validate_commit_for_task_enforces_scope_and_ancestry_invariants
      modified:
      - .forge/acdd/profile.yaml:variable:.forge.acdd.profile.roles
      - src/cli/guide.rs:function:src.cli.guide.checkpoint
      - src/cli/guide.rs:function:src.cli.guide.checkpoint_name
      - src/cli/milestone.rs:enum:src.cli.milestone.MilestoneCommand
      - src/cli/milestone.rs:function:src.cli.milestone.run
      - src/cli/mod.rs:enum:src.cli.Command
      - src/cli/mod.rs:function:src.cli.command_presentation
      - src/cli/mod.rs:impl:src.cli.Cli
      - src/cli/mod.rs:method:src.cli.Cli.run
      - src/cli/task.rs:enum:src.cli.task.BlackboardCommand
      - src/cli/task.rs:enum:src.cli.task.TaskCommand
      - src/cli/task.rs:field:src.cli.task.TaskCommand.evidence
      - src/cli/task.rs:function:src.cli.task.run
      - src/core/config.rs:function:src.core.config.load_adapter
      - src/core/config.rs:function:src.core.config.read_policy
      - src/core/config.rs:struct:src.core.config.Adapter
      - src/core/config.rs:struct:src.core.config.AdapterFile
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.rust-check-and-test
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.commands.test
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.gates.contours
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.policies
      - src/core/tasks/acdd.default.yaml:variable:src.core.tasks.acdd.default.roles
      - src/core/tasks/gates.rs:function:src.core.tasks.gates.validate_gate_proof
      - src/core/tasks/gates.rs:function:src.core.tasks.gates.validate_review
      - src/core/tasks/gates.rs:struct:src.core.tasks.gates.ReviewProof
      - src/core/tasks/mod.rs:function:src.core.tasks.Milestone.parse_specification.visit
      - src/core/tasks/mod.rs:impl:src.core.tasks.AuthoritySnapshot
      - src/core/tasks/mod.rs:impl:src.core.tasks.Milestone
      - src/core/tasks/mod.rs:method:src.core.tasks.AuthoritySnapshot.capture
      - src/core/tasks/mod.rs:method:src.core.tasks.Milestone.parse_specification
      - src/core/tasks/mod.rs:struct:src.core.tasks.AuthoritySnapshot
      - src/core/tasks/mod.rs:struct:src.core.tasks.Milestone
      - src/core/tasks/mod.rs:struct:src.core.tasks.TaskSpec
      - src/core/tasks/profile.rs:enum:src.core.tasks.profile.CommandDef
      - src/core/tasks/profile.rs:enum:src.core.tasks.profile.ProofDef
      - src/core/tasks/profile.rs:enum:src.core.tasks.profile.ProofKind
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.GateProfile.policies
      - src/core/tasks/profile.rs:field:src.core.tasks.profile.ProofDef.scheme
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.compiled
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.load_for_workspace
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.load_pinned_over
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.load_profile_path
      - src/core/tasks/profile.rs:function:src.core.tasks.profile.parse_profile
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.CommandDef
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.GateProfile
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.ProofDef
      - src/core/tasks/profile.rs:impl:src.core.tasks.profile.SchemaDef
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.CommandDef.command
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.by_stage
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.contour_set
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.gate_id
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.index_of
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.merge_override
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.GateProfile.validate
      - src/core/tasks/profile.rs:method:src.core.tasks.profile.SchemaDef.validate_value
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.GateDef
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.GateProfile
      - src/core/tasks/profile.rs:struct:src.core.tasks.profile.SchemaDef
      - src/db/tasks_store/blackboard.rs:impl:src.db.tasks_store.blackboard.TasksStore
      - src/db/tasks_store/blackboard.rs:method:src.db.tasks_store.blackboard.TasksStore.blackboard_inspect
      - src/db/tasks_store/blackboard.rs:method:src.db.tasks_store.blackboard.TasksStore.blackboard_page_for_gate
      - src/db/tasks_store/blackboard.rs:method:src.db.tasks_store.blackboard.TasksStore.blackboard_post_scoped_with_gate
      - src/db/tasks_store/blackboard.rs:method:src.db.tasks_store.blackboard.TasksStore.blackboard_read
      - src/db/tasks_store/lifecycle.rs:impl:src.db.tasks_store.lifecycle.ClaimContext
      - src/db/tasks_store/lifecycle.rs:impl:src.db.tasks_store.lifecycle.TasksStore
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.ClaimContext.claim
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.extend_scope
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.finish_resume
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.next
      - src/db/tasks_store/lifecycle.rs:method:src.db.tasks_store.lifecycle.TasksStore.release_failed_claim
      - src/db/tasks_store/mod.rs:function:src.db.tasks_store.check_stage
      - src/db/tasks_store/mod.rs:function:src.db.tasks_store.dependency_wave
      - src/db/tasks_store/mod.rs:function:src.db.tasks_store.task_not_found_message
      - src/db/tasks_store/mod.rs:function:src.db.tasks_store.verify_dirty_acdd_scope
      - src/db/tasks_store/mod.rs:impl:src.db.tasks_store.TasksStore
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.build_snapshot_candidate
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.create
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.handoff_milestone_with_unlink
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.open_project
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.set_runtime_gate
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.sync
      - src/db/tasks_store/mod.rs:method:src.db.tasks_store.TasksStore.sync_selected
      - src/db/tasks_store/mod.rs:struct:src.db.tasks_store.BlackboardMessage
      - src/db/tasks_store/recovery.rs:function:src.db.tasks_store.recovery.probe_copy
      - src/db/tasks_store/recovery.rs:function:src.db.tasks_store.recovery.probe_copy.inspect
      - src/db/tasks_store/recovery.rs:function:src.db.tasks_store.recovery.transaction
      - src/db/tasks_store/recovery.rs:impl:src.db.tasks_store.recovery.ActiveCheckpoint
      - src/db/tasks_store/recovery.rs:impl:src.db.tasks_store.recovery.Publication
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ActiveCheckpoint.parents
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.ManagedTransaction<'_>.commit
      - src/db/tasks_store/recovery.rs:method:src.db.tasks_store.recovery.Publication.publish
      - src/db/tasks_store/recovery.rs:struct:src.db.tasks_store.recovery.ManagedTransaction
      - src/db/tasks_store/recovery.rs:struct:src.db.tasks_store.recovery.Publication
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.handoff_context
      - src/engine/milestones/handoff.rs:function:src.engine.milestones.handoff.validate_effective_scope
      - src/engine/milestones/mod.rs:function:src.engine.milestones.init
      - src/engine/tasks/blackboard.rs:function:src.engine.tasks.blackboard.blackboard
      - src/engine/tasks/blackboard.rs:struct:src.engine.tasks.blackboard.BlackboardRequest
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.context_bundle
      - src/engine/tasks/context.rs:function:src.engine.tasks.context.scoped_commands
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.accepted_boundary_callers
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.candidate_snapshot
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.indexed_edge_is_current
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.indexed_seam_edges
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.validate_receipt
      - src/engine/tasks/gates.rs:function:src.engine.tasks.gates.validate_seam_reachability
      - src/engine/tasks/gates.rs:impl:src.engine.tasks.gates.TasksStore
      - src/engine/tasks/gates.rs:method:src.engine.tasks.gates.TasksStore.submit_with_pre_persist
      - src/engine/tasks/gates.rs:struct:src.engine.tasks.gates.IndexedSeamEdge
      - src/engine/tasks/git_snapshot.rs:function:src.engine.tasks.git_snapshot.capture_scoped_snapshot
      - src/engine/tasks/lifecycle.rs:function:src.engine.tasks.lifecycle.claim_inner
      - src/engine/tasks/lifecycle.rs:function:src.engine.tasks.lifecycle.finish_claim
      - src/engine/tasks/lifecycle.rs:function:src.engine.tasks.lifecycle.manage_action_name
      - src/engine/tasks/lifecycle.rs:function:src.engine.tasks.lifecycle.manage_inner
      - src/engine/tasks/lifecycle.rs:function:src.engine.tasks.lifecycle.selector_invalid_fields
      - src/engine/tasks/mod.rs:enum:src.engine.tasks.ManageAction
      - src/engine/tasks/mod.rs:function:src.engine.tasks.list
      - src/engine/tasks/mod.rs:function:src.engine.tasks.submit_inner
      - src/engine/tasks/mod.rs:struct:src.engine.tasks.Claim
      - src/engine/tasks/mod.rs:struct:src.engine.tasks.Manage
      - src/engine/tasks/workspaces.rs:impl:src.engine.tasks.workspaces.Workspace
      - src/engine/tasks/workspaces.rs:method:src.engine.tasks.workspaces.Workspace.guidance_envelope
      - src/mcp/response.rs:function:src.mcp.response.trim_page_tails
      - src/mcp/server.rs:function:src.mcp.server.same_request_adapter
      - src/mcp/tools.rs:impl:src.mcp.tools.Server
      - src/mcp/tools.rs:method:src.mcp.tools.Server.call_tool
      - src/mcp/tools.rs:method:src.mcp.tools.Server.forge_guide
      - tests/acdd/blackboard.rs:function:tests.acdd.blackboard.blackboard_milestone_profile_pin_routes_gates_and_fails_closed_on_tampering
      - tests/acdd/mcp.rs:function:tests.acdd.mcp.mcp_task_claim_guidance_includes_rules_without_schema_bloat
      - tests/acdd/mcp.rs:function:tests.acdd.mcp.stdio_tool_catalog_uses_object_schemas_for_every_property
      - tests/acdd/mcp.rs:function:tests.acdd.mcp.task_claim_bundle_and_task_manage_context_returns_unified_agent_context
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.complete_task_with_builder
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.milestone_handoff_requires_completed_tasks_and_archives_typed_receipt
      - tests/acdd/milestones.rs:function:tests.acdd.milestones.terminal_task_delivery_rolls_up_durable_context_and_prunes_blackboard
      - tests/acdd/subtasks.rs:function:tests.acdd.subtasks.cancelled_tasks_retain_history_and_do_not_unblock_dependencies
      - tests/acdd/subtasks.rs:function:tests.acdd.subtasks.task_subtasks_lifecycle_management_and_digest_independence
      - tests/mcp/context.rs:function:tests.mcp.context.repair_checkpoint_page_shapes_are_opaque_and_persistence_is_unchanged
      - tests/mcp/context.rs:function:tests.mcp.context.stdio_caps_documents_scalars_checkpoints_and_tool_errors
      - tests/mcp/context.rs:function:tests.mcp.context.stdio_server_keeps_its_linked_worktree_and_resolves_registered_sibling_names
      - tests/mcp/freshness/stdio_and_response.rs:function:tests.mcp.freshness.stdio_and_response.all_mcp_tools_admit_large_inventory_snapshot_without_raising_query_limit
      - tests/mcp/skill_sync.rs:function:tests.mcp.skill_sync.collect_skill_files
      - tests/mcp/skill_sync.rs:function:tests.mcp.skill_sync.read_skill_files
      - tests/mcp/skill_sync.rs:function:tests.mcp.skill_sync.validate_installed_skill_links
      - tests/mcp/tool_evolution/mcp_boundaries.rs:function:tests.mcp.tool_evolution.mcp_boundaries.mcp_analyze_router_checkpoint_and_guide_boundaries
      - tests/mcp/tool_evolution/mcp_boundaries.rs:function:tests.mcp.tool_evolution.mcp_boundaries.mcp_selector_and_source_boundaries
    test_suite_results:
      command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
      exit_code: 0
      tests_passed: 860
      tests_failed: 0
    automatic_project_verification: null
    linter:
      status: unobserved
    topology:
      upstream:
      - forge-mcp/forge-mcp/m-milestone-handoff-pruning-evidence-cleanup-and-list-guidance
      downstream: []
      unresolved_upstream: []
    verified_handoff_proof: aa003fa6daf2b231034cd663f1cb54267f8172b4e15134cf6659c66ac79a7319
  verification:
    command: cargo test --test commitment_integrity && cargo clippy --all-targets --all-features -- -D warnings && cargo test --all-targets
    status: passed
    tests_passed: 860
    tests_failed: 0
---

# Hierarchical ACDD gates, modular catalogs, and milestone orchestration

## Outcome and purpose

Evolve ContextUnity Forge MCP into a **Universal Hierarchical Gate Platform** where **Everything is a Gate**:

1. **Universal Gate Polymorphism**: Every step in the lifecycle is a Gate. Gates are executed either by an LLM agent (`role:`), the platform shell (`command:`), or recursively as a container (`gates:` or declarative `- tasks:`).
2. **Atomic Extended Scope Synchronization**: Eliminate scope drift. The `task extend-scope` command atomically updates SQLite and the Git markdown manifest, and the `review` gate strictly validates `extended_scope_review` before delivery.
3. **Breadcrumb-Keyed SQLite Checkpoints**: Eliminate `.forge/checkpoints.json`. Store checkpoints directly in SQLite `gate_checkpoints` keyed by the turn locator (`{milestone_id}/{gate_path}/{task_id}/{stage_id}`).
4. **Auto-Proof Submissions**: Eliminate shell JSON quoting nightmares. Automated gates (any gate configured with `proof: command`, `proof: contract`, or `proof: delivery`) require zero manual JSON arguments, while review gates (`proof: review`) accept typed cognitive review judgments.
5. **In-Place Lifecycle Operation**: Allow executing tasks directly in the current workspace or branch (`--in-place`) without forcing new worktree or branch isolation. (Native execution of plans from `docs/plans/*.md` is decoupled as milestone `AC-003`, inheriting from AC-001).
6. **Silent Telemetry (Anti-Spam Blackboard)**: Automatically log transitions and command traces into SQLite; suppress automated engine progress spam while reserving `task_blackboard` for RFCs, synthesized contract matrices, and human escalations (HITL).
7. **Seam Engine Red-to-Green Automation**: Preserve rigorous static caller and topological reachability analysis on the Contract gate, while automating the runtime Red (matching configured `expected_failure.exit_codes`) to Green (0) transition check via gate execution traces.
8. **Transition & Execution Trace Tables**: Record every lifecycle advancement in `gate_transitions` and capture command execution metrics in `gate_execution_traces`.
9. **Pre-Doc Verification, Fallback Subgraph & Autonomous Delivery**: Run the full repository check and review within `milestone_verification`, with fallback to the `build_review_cycle` subgraph on failure, and autonomous delivery commit (0 tokens) before documentation starts.
10. **Modular Subsystem Decoupling & Adaptive Guidance**: Expose configurable subsystem toggles (`codemap`, `acdd`, `blackboard`, `docs`, `checkpoints`) with preset profiles (`full`, `navigator`, `orchestrator`, `minimal`), dynamic tool registration eliminating up to 3000 tokens of schema overhead, and context-aware guidance adaptation ensuring gates and prompts function cohesively in decoupled modes.

---

## Tasks in this milestone

### Priority Waves and Execution DAG

| Wave | Priority | Task Ref | Focus & Deliverable | Dependencies |
| :--- | :--- | :--- | :--- | :--- |
| **Wave 0** | **Bootstrap** | **`upfront-contract-synthesis-and-dag-prioritization`** (Task 0) | Cross-task contract matrix, waves, error taxonomy, and disjoint scope freeze | None |
| **Wave 1** | **Foundation** | `universal-gate-ast-and-modular-catalog-spec` | Polymorphic AST serde, modular subsystem filtering, and guidance schemas | Task 0 |
| **Wave 1** | **Foundation** | `sqlite-gate-transitions-and-trace-persistence` | SQLite schema migrations (`gate_transitions`, `gate_execution_traces`, `gate_checkpoints`) | Task 0 |
| **Wave 2** | **Core Engine** | `zero-arg-transition-engine-and-command-runner` | Zero-argument submission engine, auto-proof compiler, Seam Engine Red-Green verifier, and by_pattern command runner | Wave 1 |
| **Wave 3** | **Automation** | `upfront-contract-synthesis-and-shared-helper-extraction` | Automated synthesis gate, helper extraction, and preflight milestone init | Wave 1; scope-ordered after Task 3 |
| **Wave 4** | **Runtime** | `container-gate-and-claim-engine` | In-place execution (`--in-place`), silent telemetry, state-driven claims, and unread RFC guidance injection | Wave 2 and 3 |
| **Wave 4** | **Tooling** | `scope-extension-manifest-sync-and-review-audit` | CLI zero-argument submission, atomic `task extend-scope`, manifest sync, and SQLite checkpoints tool migration | Wave 2 and 3 |
| **Wave 5** | **Quality** | `acdd-test-suite-modularization-and-domain-decomposition` | Decompose monolithic 10k-line `tests/acdd/tasks.rs` into domain modules, extract common harnesses, and establish multi-tier AGENTS.md test hierarchy | Wave 4 |

---

### Canonical cross-task contract matrix

For each row, source_task is the provider and target_task is the consumer.
Every local depends_on edge has at least one seam_dependency row. Rows start at
revision 1 with status synthesized; revision increases only when the owning
contract changes, and status becomes verified after its seam is checked. The
model and validator are
[MilestoneContractMatrix](../../src/core/tasks/mod.rs#L61),
[AcddErrorCode](../../src/core/tasks/mod.rs#L71), and
[verify_contract_matrix_parity](../../src/engine/milestones/mod.rs#L32).

| Source task | Target task | Type | Contract reference |
| :--- | :--- | :--- | :--- |
| Task 0 upfront-contract-synthesis-and-dag-prioritization | Task 1 universal-gate-ast-and-modular-catalog-spec | seam_dependency | GateProfile.error_codes |
| Task 0 upfront-contract-synthesis-and-dag-prioritization | Task 2 sqlite-gate-transitions-and-trace-persistence | seam_dependency | core::tasks::MilestoneContractMatrix |
| Task 1 universal-gate-ast-and-modular-catalog-spec | Task 3 zero-arg-transition-engine-and-command-runner | seam_dependency | GateDef; CommandDef |
| Task 1 universal-gate-ast-and-modular-catalog-spec | Task 4 upfront-contract-synthesis-and-shared-helper-extraction | seam_dependency | GateProfile; PolicyDef |
| Task 2 sqlite-gate-transitions-and-trace-persistence | Task 3 zero-arg-transition-engine-and-command-runner | seam_dependency | GateTransition; GateExecutionTrace |
| Task 2 sqlite-gate-transitions-and-trace-persistence | Task 4 upfront-contract-synthesis-and-shared-helper-extraction | seam_dependency | TasksStore::replace_contract_matrix; TasksStore::list_contract_matrix |
| Task 3 zero-arg-transition-engine-and-command-runner | Task 5 container-gate-and-claim-engine | seam_dependency | execute_command_gate; compile_evidence_envelope |
| Task 4 upfront-contract-synthesis-and-shared-helper-extraction | Task 5 container-gate-and-claim-engine | seam_dependency | synthesize_milestone_contracts; validate_wave_dag |
| Task 3 zero-arg-transition-engine-and-command-runner | Task 6 scope-extension-manifest-sync-and-review-audit | seam_dependency | compile_evidence_envelope |
| Task 4 upfront-contract-synthesis-and-shared-helper-extraction | Task 6 scope-extension-manifest-sync-and-review-audit | seam_dependency | synthesize_milestone_contracts |
| Task 5 container-gate-and-claim-engine | Task 7 acdd-test-suite-modularization-and-domain-decomposition | seam_dependency | tests/acdd/seam_reachability.rs |
| Task 6 scope-extension-manifest-sync-and-review-audit | Task 7 acdd-test-suite-modularization-and-domain-decomposition | seam_dependency | tests/acdd/scope_sync.rs |

verify_contract_matrix_parity rejects a wrong milestone identity, unknown task
references, duplicate row keys, empty references, non-positive revisions, and
any local dependency without the directed provider-to-consumer seam_dependency
entry. Additional shared_helper and shared_type rows may describe contracts
that do not correspond to manifest dependencies.
Task 2's replace_contract_matrix writes the full matrix atomically; its
list_contract_matrix read returns entries ordered by source_task, target_task,
and contract_ref.

The manifest task order is the stable tie-breaker within each topological wave.
depends_on remains the source of execution blockers; a task may run only after
all earlier-wave dependencies pass.

| Wave | Concurrent task pair | Disjoint ownership |
| :--- | :--- | :--- |
| 0 | Task 0 | Owns the bootstrap contract, tests/acdd/mod.rs, and reserved seam files before later waves start. |
| 1 | Tasks 1 and 2 | Task 1 owns core/profile/CLI/MCP/engine sources and gates_ast.rs; Task 2 owns src/db/tasks_store/ and persistence.rs. |
| 2 | Task 3 | Owns transition/command-runner, task-store lifecycle, module registration seams, runner.rs, and task reference updates. |
| 3 | Task 4 | Owns synthesis sources and synthesis.rs; shared store/module files are serialized after Task 3 by the scope dependency. |
| 4 | Tasks 5 and 6 | Task 5 owns claim/container, reader, context guidance, and seam_reachability.rs; Task 6 owns scope-sync, handoff, CLI/MCP, governance documents, and scope_sync.rs. |
| 5 | Task 7 | Owns tests/acdd/tasks/ domain decomposition, tests/acdd/tasks.rs retirement, tests/acdd/mod.rs wiring, tests/acdd/support.rs fixtures, tests/acdd/AGENTS.md, tests/acdd/tasks/AGENTS.md, tests/AGENTS.md, and docs/testing/README.md. |

Same-wave tasks have no shared write path. Cross-wave path reuse is intentional
and serialized by the dependency graph.

---

### task: upfront-contract-synthesis-and-dag-prioritization

```yaml
task_ref: upfront-contract-synthesis-and-dag-prioritization
target: Synthesize cross-task contract matrix, establish deterministic topological execution waves, standardize shared data models & error variants, pre-register seam test harness modules in tests/acdd/mod.rs, and freeze disjoint scopes for all AC-001 tasks
proof_policy: seam-test-first
contract_revision: 1
depends_on: []
scope:
- docs/milestones/AC-001-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/future/references/acdd_profile.v2.yaml.example
- src/core/tasks/mod.rs
- src/engine/milestones/mod.rs
- tests/acdd/mod.rs
- tests/acdd/contract_synthesis.rs
- tests/acdd/gates_ast.rs
- tests/acdd/persistence.rs
- tests/acdd/runner.rs
- tests/acdd/synthesis.rs
- tests/acdd/seam_reachability.rs
- tests/acdd/scope_sync.rs
status: completed
subtasks:
- subtask_ref: cross-task-data-contract-matrix
  title: Define canonical cross-task contract matrix and shared data structures
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: deterministic-priority-waves-dag
  title: Establish deterministic topological execution waves and explicit DAG dependencies
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: seam-test-harness-pre-registration
  title: Pre-register seam test stub files in tests/acdd/mod.rs and freeze test module boundaries
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: disjoint-scope-boundaries-freeze
  title: Freeze disjoint file write scopes and seam test allocations across all tasks
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
- subtask_ref: shared-error-taxonomy-and-diagnostic-codes
  title: Standardize shared error variants and diagnostic error codes in ADR and reference profile
  status: completed
  evidence: Focused contract seam and full `cargo test --test acdd` (117 passed, 0 failed); `cargo clippy --all-targets --all-features -- -D warnings` passed.
receipt:
  commit: a456afb7f039b07168fca6fa223b49cb28916156
  contract_revision: 1
  passed_at: 2026-10-24T03:01:11.598244668+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 117
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: I inspected the claim-bound candidate snapshot 3d30822b78544a627df91842ec919474ed098e0f and the worktree status. The changed/untracked paths are exactly the 13 paths in the initial Task 0 scope; no scope extensions or unrelated configuration, credential, generated-state, or neighboring-project paths are present.
        claims:
          applicable: true
          evidence: The accepted contract matrix, dependency waves, stable error catalog, and reserved seam modules match the Task 0 contract. The production validator verify_contract_matrix_parity checks matrix/milestone identity, task references, row uniqueness, contract references/revisions, and dependency-row parity; tests/acdd/contract_synthesis.rs calls that production validator for malformed and valid cases. The cycle diagnostic is stable. The accepted build proof for this exact snapshot records strict Clippy plus cargo test --test acdd passing (117 passed, 0 failed).
        concurrency:
          applicable: false
          evidence: Task 0 adds value types and a pure parity validator; it does not add shared-state writes, transactions, locks, claims, or receipt publication. Atomic full-matrix replacement is explicitly assigned to the dependent Task 2 TasksStore API, so there is no Task 0 concurrent writer or transaction boundary to review.
        project_isolation:
          applicable: true
          evidence: The matrix is manifest-local and validated against one parsed milestone. Persistence is assigned to TasksStore::replace_contract_matrix/list_contract_matrix. Existing TasksStore::open_project binds repository and project, and milestone_identity qualifies a milestone id with that store's namespace; the matrix value therefore need not carry a portable repository/project identity under the accepted contract.
        administration:
          applicable: true
          evidence: The candidate changes a future profile example and future ADR only; it changes no active profile/configuration, migration, hook, receipt, or delivery path. The existing profile/invariant proof-shape mismatch is a deferred Task 6 item and is not introduced by this Task 0 candidate.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: Deferred to Task 6 (scope-extension-manifest-sync-and-review-audit): AC-001 invariant INV-EXTENDED-SCOPE-MANIFEST-SYNC says review proof must validate extended_scope_review approval, but the current active ReviewProof schema only accepts `decision` and the five contour results, and rejects that field. Task 0 has no scope extensions. Task 6 must align the typed review proof/schema and add production-seam coverage before milestone handoff.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: not_applicable
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 0)
- `doc:milestone_contract_matrix`
- `doc:shared_error_taxonomy`
- `fn:verify_contract_matrix_parity`

- `enum:ContractType`
- `enum:ContractStatus`
- `struct:MilestoneContractEntry`
- `struct:MilestoneContractMatrix`
- `enum:AcddErrorCode`
- `struct:AcddError`
- `method:AcddErrorCode::as_str`
- `method:AcddError::new`

### task: universal-gate-ast-and-modular-catalog-spec

```yaml
task_ref: universal-gate-ast-and-modular-catalog-spec
target: 'Define AST data structures, serde schemas, and field access validators for universal polymorphic gates, policies catalog, reject_routing (scope -> build, symbols -> contract), symmetrical scope and universal symbols boundaries (kind:identifier), guidance slot binding schema (guidance: { slot: key }), manifest-to-gate slot validation with unbound warnings, detailed hitl guidance catalog (hitl.careless, hitl.curious, hitl.pro, hitl.paranoic), by_pattern command dispatch, two-tier scopes, modular subsystem registry with dynamic tool filtering, 96 KiB response budget compliance for modular tools, manifest linked_to milestone metadata, task-level cross-milestone dependencies (task_depends_on), speculative concurrency schema (mode: speculative, sync_on: snapshot), linked milestone task status context injection, composable persistence policies (pre/post/immutable/git_commit_note), on-demand snapshot and rollback safety-net tools, payload overflow limits (git_commit_note.limits.max_bytes: 2048), and architect DAG wave optimization'
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- upfront-contract-synthesis-and-dag-prioritization
scope:
- src/core/config.rs
- src/core/response.rs
- src/core/tasks/mod.rs
- src/core/tasks/profile.rs
- src/core/tasks/gates.rs
- src/core/tasks/catalog.rs
- src/core/tasks/manifest.rs
- src/core/tasks/acdd.default.yaml
- src/cli/mod.rs
- src/cli/validate.rs
- src/mcp/server.rs
- src/mcp/response.rs
- src/mcp/tools.rs
- src/engine/tasks/mod.rs
- src/engine/tasks/context.rs
- tests/acdd/gates_ast.rs
- docs/future/references/acdd_profile.v2.yaml.example
- skills/contextunity-forge/references/acdd_profile.yaml.example
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
status: completed
subtasks:
- subtask_ref: universal-gate-ast-serde
  title: Implement universal polymorphic GateDef AST data structures with Serde serialization
  status: ready
  evidence: null
- subtask_ref: zero-language-hardcode-profile-invariant
  title: Enforce INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE in embedded default profile
  status: ready
  evidence: null
- subtask_ref: fail-fast-document-validator-cli
  title: Implement fail-fast CLI document and manifest schema validator command
  status: ready
  evidence: null
- subtask_ref: command-catalog-by-pattern-schema
  title: Define declarative command catalog schema with by_pattern polyglot dispatch
  status: ready
  evidence: null
- subtask_ref: policies-catalog-schema-and-enforcement-validator
  title: Define policies catalog schema and machine-level gate enforcement validators
  status: ready
  evidence: null
- subtask_ref: two-tier-scope-governance-schema
  title: Define two-tier scope governance schema (scope and extended_scope) in TaskSpec
  status: ready
  evidence: null
- subtask_ref: reject-routing-and-symbols-confinement-schema
  title: Implement reject_routing and universal symbols (kind:identifier) schema in gate definitions
  status: ready
  evidence: null
- subtask_ref: guidance-slot-binding-and-hitl-catalog
  title: Implement guidance slot binding schema and detailed 4-tier HITL guidance catalog
  status: ready
  evidence: null
- subtask_ref: gate-field-access-matrix-validator
  title: Implement gate driver field validation matrix enforcing mutually exclusive gate kinds
  status: ready
  evidence: null
- subtask_ref: unified-guidance-catalog-schema
  title: Define modular guidance catalog schema and dynamic prompt compilation rules
  status: ready
  evidence: null
- subtask_ref: required-skills-manifest-serde-and-validator
  title: Add required_skills frontmatter serde schema and validator to MilestoneManifest
  status: ready
  evidence: null
- subtask_ref: multi-repo-manifest-repositories-serde
  title: Implement multi-repository coordination schema and repositories manifest field
  status: ready
  evidence: null
- subtask_ref: speculative-concurrency-depends-on-schema
  title: Define speculative concurrency mode and snapshot-sync dependencies schema
  status: ready
  evidence: null
- subtask_ref: modular-subsystem-registry-and-dynamic-tool-filter
  title: Implement modular subsystem registry with dynamic MCP tool registration and filtering
  status: ready
  evidence: null
- subtask_ref: context-aware-guidance-adapter
  title: Implement context-aware workflow guidance adapter dynamically adapting to enabled subsystems
  status: ready
  evidence: null
- subtask_ref: modular-subsystem-response-adaptation
  title: Ensure modular tool responses strictly comply with 96 KiB response budget
  status: ready
  evidence: null
- subtask_ref: manifest-linked-milestones-and-task-dependencies-schema
  title: Define linked_to milestones and cross-milestone task_depends_on schema
  status: ready
  evidence: null
- subtask_ref: linked-milestones-context-and-guidance-injection
  title: Implement context injection for linked milestone task status and blockers
  status: ready
  evidence: null
- subtask_ref: composable-persistence-policy-schema
  title: Implement composable persistence policy schema (pre/post/immutable/git_commit_note)
  status: ready
  evidence: null
- subtask_ref: payload-overflow-limits-and-git-commit-note-schema
  title: Implement 2 KiB payload overflow guard and git commit note schema
  status: ready
  evidence: null
- subtask_ref: ondemand-snapshot-and-rollback-tools
  title: Implement on-demand task snapshot and rollback tool schemas and safety nets
  status: ready
  evidence: null
- subtask_ref: architect-synthesis-dag-optimizer
  title: Implement topological wave optimization schema
  status: ready
  evidence: null
- subtask_ref: promote-future-profile-v2-example
  title: Promote docs/future/references/acdd_profile.v2.yaml.example to skills/contextunity-forge/references/acdd_profile.yaml.example after gate AST validation passes
  status: ready
  evidence: null
receipt:
  contract_revision: 1
  passed_at: 2026-10-24T06:53:11.028570505+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 120
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Candidate 25e77d96c4703fbec0c71722d53322fb015e3348 matches the current worktree; all 21 snapshot blobs are in the admitted Task1 scope/extensions; git diff --check is clean.
        claims:
          applicable: true
          evidence: 'The candidate closes all historical implementation findings. The latest three paths are covered by production MCP seams: target adapter governs calls in both configuration directions; nonbundled task_claim and task_manage inspect prune disabled blackboard guidance; docs-only query removes codemap recovery advice while retaining docs pages. Earlier validator, modular registry, universal profile, named snapshot/rollback, and context preflight findings remain covered by source and existing tests.'
        concurrency:
          applicable: true
          evidence: Direct task_claim and task_manage(next) preflight disabled requirements before store mutation; production seam verifies task claim state and planned milestone state remain unchanged. Named rollback retains per-worktree locking and HEAD/index/worktree fencing.
        project_isolation:
          applicable: true
          evidence: call_tool resolves and loads the selected worktree adapter before disabled-tool checks; MCP seam proves primary full/selected minimal rejects and primary minimal/selected full succeeds. Task context uses owning workspace module settings.
        administration:
          applicable: true
          evidence: Profile and CLI validators, named task_manage snapshot/rollback, and receipt delivery boundaries remain intact. No receipt, migration, hook, credential, or protected milestone changes are present. Two reference-page findings are deferred to the milestone documentation scan.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: {"candidate_baseline_head":"ad339da22a33bf1feb7e5ee479ec5940e048fa8c","finding":"CLI task actions are modeled separately in src/cli/task.rs; adding snapshot/rollback there would expose the same API from CLI.","evidence":"Forge task_manage(action=extend_scope, paths=[src/cli/task.rs]) returned TASK_SCOPE_CONFLICT because that path belongs to scope-extension-manifest-sync-and-review-audit.","deferred_work":"Keep Task1''s admitted MCP task_manage snapshot/rollback surface complete. Route CLI task snapshot/rollback through the owning scope-extension-manifest-sync-and-review-audit task when it becomes eligible, or reconcile during milestone docs/integration if its contract includes that surface.","next_owner":"scope-extension-manifest-sync-and-review-audit","next_action":"Confirm CLI task command surface when that task claims its CLI scope."}'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 1)
- `struct:GateDef`
- `struct:GateProfile`
- `struct:PolicyDef`
- `struct:CommandDef`
- `struct:GuidanceSlotBinding`
- `struct:RejectRouting`
- `struct:SubsystemRegistry`
- `fn:validate_gate_profile`
- `fn:resolve_guidance_slot`
- `fn:filter_subsystem_tools`
- `cmd:validate`
- `slot:hitl`

- field:GateProfile.error_codes

### task: sqlite-gate-transitions-and-trace-persistence

```yaml
task_ref: sqlite-gate-transitions-and-trace-persistence
target: Implement SQLite schema migrations for gate_transitions, gate_execution_traces, gate_checkpoints (breadcrumb-keyed), milestone_contract_matrix, and task_blackboard recipient addressing (recipient_task_id) for inter-agent RFC negotiation
proof_policy: seam-test-first
contract_revision: 1
depends_on:
- upfront-contract-synthesis-and-dag-prioritization
scope:
- src/db/tasks_store/mod.rs
- src/db/tasks_store/schema.rs
- src/db/tasks_store/lifecycle.rs
- src/db/tasks_store/matrix.rs
- src/db/tasks_store/trace.rs
- tests/acdd/persistence.rs
subtasks:
- subtask_ref: gate-transitions-table-migration
  title: Implement SQLite migration and store APIs for durable gate_transitions audit table
  status: ready
  evidence: null
- subtask_ref: gate-execution-trace-table-migration
  title: Implement SQLite migration for gate_execution_traces with 2 KiB output snippets
  status: ready
  evidence: null
- subtask_ref: gate-checkpoints-breadcrumb-keyed-table
  title: Implement gate_checkpoints SQLite table keyed by compound turn breadcrumb
  status: ready
  evidence: null
- subtask_ref: milestone-contract-matrix-table
  title: Implement milestone_contract_matrix SQLite table for cross-task helper tracking
  status: ready
  evidence: null
- subtask_ref: blackboard-recipient-and-rfc-support
  title: Add recipient_task_id column and status indexing to task_blackboard table
  status: ready
  evidence: null
status: completed
receipt:
  contract_revision: 1
  passed_at: 2026-10-24T08:44:07.263926214+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 121
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'The reopened repair changes only src/db/tasks_store/mod.rs, src/db/tasks_store/blackboard.rs, and tests/acdd/persistence.rs against baseline 5dbbcbec22848d936b0d6af2217a40055cfafcea. All nine paths in scoped candidate 6047fabd10626ad3b40802fb28da05346a3522b5 belong to Forge allowed_write_scope, and the complete allowed scope is unchanged from that candidate in the worktree. extended_scope_review: APPROVED for the Forge-authorized extension src/db/tasks_store/blackboard.rs, the canonical persistence owner for recipient posting, inbox paging, and acknowledgement. Other task paths retain accepted content; concurrent coordinator documentation edits are outside this scoped candidate. No protected configuration, credentials, generated receipt, or neighboring project file belongs to this repair.'
        claims:
          applicable: true
          evidence: Contract draft blackboard message 1146 requires recipient_task_id independently of task_id, store posting/inspection round-trip, and same-project/same-milestone validation. BlackboardMessage adds an optional backward-compatible serialized field, and all row decoders return it. Posting preserves owning hierarchy scope. Recipient pages select active messages for the validated milestone/task with bounded payload-free summaries in insertion order. Acknowledgement matches recipient, milestone, status, and message id. The additive real TasksStore seam at tests/acdd/persistence.rs:148-220 proves persistence, serialization, unread summaries, intended-recipient acknowledgement, repeated/wrong-recipient rejection, and foreign-project rejection.
        concurrency:
          applicable: true
          evidence: Posting uses one conditional INSERT SELECT that rechecks owner, optional subtask, recipient milestone, and non-completed owner/recipient states within the SQLite write, including recipient-only milestone posts. This preserves the existing completion-race fence. Acknowledgement is a single conditional UPDATE from active to read, so competing acknowledgements cannot both succeed. The repair adds no filesystem/Git mutation, lock order, receipt publication, or task revision change.
        project_isolation:
          applicable: true
          evidence: Posting, recipient paging, and acknowledgement use TasksStore::assert_id/inspect, the repository/project task namespace, and milestone_scope_ref. Posting and inbox reads reject recipient/milestone mismatch; insert SQL independently checks persisted milestone identity. Acknowledgement matches the namespace-qualified milestone and recipient. SQL values are parameter bound. Existing inspect/read/page APIs retain their project/owner scope while decoding recipient_task_id. No workspace, symlink, profile, or path authority behavior changes.
        administration:
          applicable: true
          evidence: 'The repair leaves accepted version-7 migration, recipient/status columns and index, profile pins, hooks, receipts, task authority, and gate lifecycle untouched. Existing posts retain recipient None. Legacy messages remain decodable via serde(default), and absent recipients remain omitted. Accepted builder proof: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd; exit 0, 121 passed, 0 failed. Independent review verification: cargo test --test acdd; exit 0, 121 passed, 0 failed (352.01 s).'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 1)
- `table:gate_transitions`
- `table:gate_execution_traces`
- `table:gate_checkpoints`
- `table:milestone_contract_matrix`
- `column:task_blackboard.recipient_task_id`
- `struct:GateTransition`
- `struct:GateExecutionTrace`
- `struct:GateCheckpoint`
- `fn:record_transition`
- `fn:record_trace`
- `fn:save_checkpoint`
- `fn:TasksStore::record_gate_transition`
- `fn:TasksStore::record_execution_trace`
- `fn:TasksStore::save_checkpoint`

- `fn:TasksStore::replace_contract_matrix`
- `fn:TasksStore::list_contract_matrix`

### task: zero-arg-transition-engine-and-command-runner

```yaml
task_ref: zero-arg-transition-engine-and-command-runner
target: Implement the zero-argument task submission engine and auto-proof compiler from gate execution traces, Seam Engine Red-to-Green automated transition verifier, autonomous command runner with by_pattern multi-language dispatch, speculative dependency runner with upstream snapshot auto-rebase, and automated task telemetry passport git commit note generator adhering to INV-PAYLOAD-OVERFLOW-GUARD (2 KiB note limit, compact Markdown diff/metrics, complete traces to disk logs)
proof_policy: seam-test-first
contract_revision: 2
depends_on:
- universal-gate-ast-and-modular-catalog-spec
- sqlite-gate-transitions-and-trace-persistence
scope:
- src/engine/tasks/submit.rs
- src/engine/milestones/command_gate.rs
- src/engine/tasks/scheduler.rs
- src/db/tasks_store/lifecycle.rs
- src/db/tasks_store/mod.rs
- src/engine/tasks/mod.rs
- src/engine/milestones/mod.rs
- tests/acdd/runner.rs
- docs/reference/tasks.md
status: completed
subtasks:
- subtask_ref: zero-arg-proof-compiler
  title: Implement zero-argument proof payload compiler assembling traces and candidate trees
  status: completed
  evidence: 'Claim-bound proof compilation verifies latest complete run, candidate, claim, and registered command identity. Cross-project isolation regression rejects foreign run IDs through all four store APIs. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: seam-engine-red-green-verifier
  title: Implement Seam Engine transition verifier proving red baseline before green admission
  status: completed
  evidence: 'Red-to-green acceptance requires same verified seam, claim/run provenance, distinct gate labels, and red-before-green chronology; focused seam covers mismatched seam and reversed order. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: autonomous-command-executor-daemon
  title: Implement autonomous command runner daemon executing shell commands with 0 LLM tokens
  status: completed
  evidence: 'Command runner persists reserved attempts and full logs, terminates process groups, and bounds pipe reader completion. R5 regression rejects a .forge symlink without creating the outside task-traces directory. R7 fixed cfg imports: fs/OpenOptions are cfg(not(unix)); File is cfg(unix). Only x86_64-unknown-linux-gnu is installed, so a non-Unix target compile was unavailable; source branch/import pairing was verified structurally after scan. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: multi-language-by-pattern-command-evaluator
  title: Implement by_pattern polyglot scope matcher evaluating commands sequentially
  status: completed
  evidence: 'Sequential by_pattern dispatch evaluates matching paths using language-neutral profile types and preserves command order. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: failure-back-routing-evaluator
  title: Implement reject_routing evaluator routing scope to build gate and symbols to contract gate
  status: completed
  evidence: 'Pure reject-routing resolver proves scope -> build and symbol -> contract; persisted routing stays assigned to Task5 because gates.rs is outside Task3 scope. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: automated-telemetry-passport-note-writer-and-overflow-guard
  title: Implement automated git commit note writer enforcing 2 KiB bounded Markdown payload
  status: completed
  evidence: 'Git telemetry passport note stays <=2 KiB while complete execution logs remain persisted. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
- subtask_ref: speculative-dependency-snapshot-rebase-runner
  title: Implement speculative dependency runner auto-rebasing branches on snapshot updates
  status: completed
  evidence: 'Speculative snapshot rebase validates a clean worktree and safely restores branch/worktree on failure. Focused runner seam: 1 passed, 0 failed. `cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd`: exit 0; Clippy passed; ACDD 124 passed, 0 failed, 0 ignored in 497.61s. Scan succeeded and fresh code_map_explain proves runner-to-production edges (13 symbols, 35 call occurrences, no analysis gaps).'
receipt:
  commit: e2956f654f51a8fe5bd7d97d529577a582f059e7
  contract_revision: 2
  passed_at: 2026-10-24T16:35:04.039804165+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 124
      tests_failed: 0
      log: 'R7 fix is limited to cfg imports in command_gate.rs: File is imported only under cfg(unix); std::fs::{self, OpenOptions} is imported under cfg(not(unix)), matching the non-Unix create_confined_log branch''s fs::symlink_metadata, fs::create_dir, and OpenOptions uses. rustup target list --installed reported only x86_64-unknown-linux-gnu, so a non-Unix cross-target compile was unavailable; the conditional import/branch pairing was checked structurally after scan. Focused Task3 seam passed: 1 passed, 0 failed, 123 filtered out. Strict lane exited 0: cargo clippy --all-targets --all-features -- -D warnings passed, then cargo test --test acdd reported 124 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 497.61s. Refreshed scan completed successfully. Fresh code_map_explain shows runner outbound edges to 13 production symbols (35 call occurrences), including execute_command_gate, compile_evidence_envelope, submit_zero_arg, verify_red_to_green, and evaluate_reject_routing, with no analysis gaps; create_confined_log source is indexed with no analysis gaps. R5/R6 behavior remains covered by the focused seam: outside .forge symlink creates no task-traces child, and all four foreign project run-ID APIs reject with TASK_PROJECT_MISMATCH. rustfmt --edition 2024 --check src/engine/milestones/command_gate.rs and git diff --check passed.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: All nine admitted snapshot blobs match the worktree; authorized manifest adjustment unchanged; no protected source or receipt changes.
        claims:
          applicable: true
          evidence: R1-R6 remain closed; R7 cfg(not(unix)) std::fs imports are now correctly scoped.
        concurrency:
          applicable: true
          evidence: Atomic attempts, exclusive logs, process-group timeout and bounded readers remain; regression evidence verified.
        project_isolation:
          applicable: true
          evidence: Guarded traversal precedes mutation; all four run-ID paths enforce project ownership and two-project tests cover them.
        administration:
          applicable: true
          evidence: 'No administration issues. Task6 pruning of both provenance tables is recorded in handoff #1171; wrapper graph-edge limitation is non-blocking.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: Task 6 handoff: when implementing the CLI zero-argument task lifecycle / milestone handoff cleanup, prune the Task 3 provenance rows in gate_execution_runs and gate_execution_attempts together with their associated execution traces. These tables are created by the Task 3 task-store schema; this cleanup remains intentionally deferred to Task 6 ownership.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 2)
- `fn:execute_command_gate`
- `fn:compile_evidence_envelope`
- `fn:evaluate_reject_routing`
- `fn:TasksStore::submit_zero_arg`

### task: upfront-contract-synthesis-and-shared-helper-extraction

```yaml
task_ref: upfront-contract-synthesis-and-shared-helper-extraction
target: Implement milestone contract synthesis gate with cross-task shared helper extraction, parallel seam decoupling, blackboard matrix publication, targeted task_blackboard messaging with recipient_task_id (role/task/path ownership addressing), and preflight lifecycle initialization (renaming template scaffolding to milestone draft, while milestone init validates manifest, checks required_skills, coordinates repositories, and scaffolds worktrees)
proof_policy: seam-test-first
contract_revision: 5
depends_on:
- universal-gate-ast-and-modular-catalog-spec
- sqlite-gate-transitions-and-trace-persistence
scope:
- src/engine/milestones/contract.rs
- src/engine/milestones/synthesis.rs
- src/engine/milestones/mod.rs
- src/engine/tasks/blackboard.rs
- src/cli/milestone.rs
- tests/acdd/synthesis.rs
- docs/reference/cli.md
- src/cli/task.rs
- src/db/tasks_store/blackboard.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/matrix.rs
scope_roots:
- src/engine/milestones/contract.rs
- src/engine/milestones/synthesis.rs
- src/engine/milestones/mod.rs
- src/engine/tasks/blackboard.rs
- src/cli/milestone.rs
- tests/acdd/synthesis.rs
- docs/reference/cli.md
- src/cli/task.rs
- src/db/tasks_store/blackboard.rs
- src/db/tasks_store/mod.rs
- src/db/tasks_store/matrix.rs
- src/engine/linker/resolution.rs
- tests/languages/profiles.rs
status: completed
subtasks:
- subtask_ref: shared-helper-extraction-validator
  title: Implement upfront shared helper and seam dependency extraction validator
  status: ready
  evidence: null
- subtask_ref: parallel-disjoint-scope-analyzer
  title: Implement static scope disjointness analyzer validating same-wave tasks
  status: ready
  evidence: null
- subtask_ref: synthesis-blackboard-matrix-publisher
  title: Implement contract matrix publisher posting synthesized contracts to task_blackboard
  status: ready
  evidence: null
- subtask_ref: blackboard-recipient-router-and-dispatcher
  title: Implement targeted blackboard router dispatching messages by recipient_task_id
  status: ready
  evidence: null
- subtask_ref: milestone-draft-rename-and-preflight-init-scaffolding
  title: Implement milestone init preflight validation, manifest verification, and worktree scaffolding
  status: ready
  evidence: null
receipt:
  commit: 34ee2133330bf8deae1edf4a94d97ffec744c66c
  contract_revision: 5
  passed_at: 2026-10-24T12:46:06.542426037+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 123
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Independent review of build snapshot 45d6725f64d715b3ea7b4e6efa69ce8e39ad952f against base 4c78414680da53df8514903b3ba8d66597cc4b10 confirmed all 11 changed paths are admitted by contract revision 5; src/engine/linker/resolution.rs and tests/languages/profiles.rs are absent from the diff.
        claims:
          applicable: true
          evidence: The milestone preflight reaches TasksStore::sync_with_contract_matrix_and_initial_message. Task rows, contract matrix, and initial broadcast are written through the same ManagedTransaction; tests inject both matrix and broadcast insert failures and assert no persisted rows.
        concurrency:
          applicable: true
          evidence: Task sync, matrix replacement, and initial message insertion complete before the unified transaction commits. Failure-injection seam assertions cover zero rows across tasks, milestone_contract_matrix, and task_blackboard, and cleanup of the created worktrees.
        project_isolation:
          applicable: true
          evidence: Reviewer traced milestone and participant path validation, including symlink/traversal rejection and canonical workspace boundaries; targeted blackboard delivery validates source and recipient task IDs within the selected milestone.
        administration:
          applicable: true
          evidence: 'Preflight validates required skills, module/profile requirements, and SQLite revision bounds before worktree or database side effects. Git subprocess calls use core::git. Strict Clippy and the scoped ACDD suite passed: 123 passed, 0 failed.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 2)
- `fn:synthesize_milestone_contracts`
- `fn:extract_shared_helpers`
- `fn:validate_wave_dag`
- `struct:MilestoneContractMatrix`
- `cmd:milestone init`
- `cmd:milestone draft`

### task: container-gate-and-claim-engine

```yaml
task_ref: container-gate-and-claim-engine
target: Implement hierarchical container gate state machine advancing through child gates, with in-place execution (--in-place), silent engine telemetry, state-driven zero-flag claims and breadcrumbs, unread blackboard RFC guidance injection into task claim guidance, and precise static seam reachability with stale-index and test-helper diagnostics
proof_policy: seam-test-first
contract_revision: 3
depends_on:
- zero-arg-transition-engine-and-command-runner
- upfront-contract-synthesis-and-shared-helper-extraction
scope:
- src/engine/tasks/context.rs
- src/engine/tasks/claim.rs
- src/engine/tasks/gates.rs
- src/db/reader.rs
- tests/acdd/seam_reachability.rs
- docs/reference/acdd.md
- src/engine/tasks/mod.rs
- src/engine/tasks/lifecycle.rs
- tests/acdd/tasks.rs
status: completed
subtasks:
- subtask_ref: nested-subgate-state-machine
  title: Implement hierarchical container gate state machine advancing through nested child gates
  status: ready
  evidence: null
- subtask_ref: in-place-lifecycle-executor
  title: Implement in-place task execution (--in-place) bypassing worktree cloning for single-task runs
  status: ready
  evidence: null
- subtask_ref: silent-telemetry-blackboard-filter
  title: Filter automated engine telemetry from task_blackboard, preserving human blockers and RFCs
  status: ready
  evidence: null
- subtask_ref: automated-scoped-verification-runner
  title: Implement scoped verification runner executing matching commands on task build submission
  status: ready
  evidence: null
- subtask_ref: agent-breadcrumb-formatter
  title: Format locator breadcrumbs ({milestone_id}/{gate_path}/{task_id}/{stage_id}) injected during task claim
  status: ready
  evidence: null
- subtask_ref: admit-candidate-seams-from-resolution-coverage
  title: Inspect indexed coverage and canonical import/re-export bindings when direct call edges are absent; require a proven in-scope production connection before admission
  status: ready
  evidence: null
- subtask_ref: distinguish-stale-index-drift-with-actionable-error
  title: Report TASK_SEAM_STALE with an index-refresh action when source drift invalidates the indexed seam instead of misreporting TASK_SEAM_DISCONNECTED
  status: ready
  evidence: null
- subtask_ref: support-multi-hop-test-fixture-harnesses
  title: Trace up to four hops through verified test helpers or fixtures to the in-scope production target without widening production scope to include test helpers
  status: ready
  evidence: null
- subtask_ref: provide-actionable-seam-diagnostics-in-gate-errors
  title: Report discovered test calls, coverage statuses, reached targets, and why static seam traversal stopped
  status: ready
  evidence: null
- subtask_ref: workflow-guidance-blackboard-rfc-injector
  title: Inject unread task-targeted and RFC blackboard messages into task claim guidance
  status: ready
  evidence: null
receipt:
  contract_revision: 3
  passed_at: 2026-10-24T20:36:49.277953074+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 125
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: 'Independent reviewer compared candidate 397468a10cfc149aec471d10940ea77e84535885 with the task record: all nine admitted paths are represented, including tests/acdd/tasks.rs and the approved module/lifecycle extensions. Scoped change analysis found only admitted task engine and ACDD test paths; no extra staged paths or post-snapshot edits were recorded.'
        claims:
          applicable: true
          evidence: Independent reviewer traced nested leaf selection through claim_inner and cursor advancement on pass, recipient-targeted blackboard guidance, and four-hop current-source seam validation with stale-index diagnostics. ACDD tests exercise real task submission/storage paths and direct, multi-hop, and stale-source cases.
        concurrency:
          applicable: true
          evidence: Independent reviewer checked transactional submission/recovery, reload and stage/profile validation, claim/contract revision fencing, worker/worktree checks, idempotency, and rollback of provisional manifest writes. No partial-write or stale-authority defect was found.
        project_isolation:
          applicable: true
          evidence: Independent reviewer verified workspace ownership, worktree, authority, scope, and confined-path checks before claim, source reads, and submission mutations. No cross-project or path-escape defect was found.
        administration:
          applicable: true
          evidence: 'Candidate does not alter schema migrations, profile configuration, or protected milestone authority. Build evidence is bound to candidate 397468a with strict Clippy success and ACDD 125/125. The independent review noted one non-blocking unresolved indexed call edge; it is durably recorded as deferred message #1184.'
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes:
    - 'deferred: Non-blocking follow-up from independent Task 5 review: the refreshed code map still does not resolve the direct source call `lifecycle.rs::claim_inner -> claim_nested_gate` as an indexed call edge, though the source contains it and graph freshness is matched. The reviewer verified the direct source path and production-seam tests; all five review contours pass and the full ACDD target is green. Track as an index call-resolution coverage gap with its owning indexing work, not as a Task 5 blocker.'
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 3)
- `fn:claim_nested_gate`
- `fn:advance_gate_cursor`
- `struct:GateCursor`
- `fn:resolve_subagent_role`
- `cmd:task claim`

#### Static seam verification acceptance

1. A current, proven test call through a canonical import or re-export reaches
   the in-scope production target without a false `TASK_SEAM_DISCONNECTED`.
   Coverage expressions identify candidates for verification; an unresolved
   name match or successful compilation alone does not establish that connection.
2. Source changes that invalidate indexed call or import evidence produce
   `TASK_SEAM_STALE` and an explicit refresh action when no other current proven
   path exists. Preserve configured profile admission rules.
3. A bounded path of up to four hops through test helpers reaches its verified
   production target; unrelated or genuinely disconnected tests remain rejected.
4. Rejections show the relevant calls, statuses, reached intermediate targets,
   and stop reason. Verify these cases in the existing shared ACDD task seam and
   update task/ACDD reference documentation with the implementation.

### task: scope-extension-manifest-sync-and-review-audit

```yaml
task_ref: scope-extension-manifest-sync-and-review-audit
target: Wire zero-argument task submission into the CLI, implement atomic task extend-scope manifest synchronization and extended_scope_review enforcement, migrate session checkpoints to SQLite, prune milestone gate telemetry on handoff, and implement the pre-documentation milestone verification container with a full-repository check, independent review, bounded build_review_cycle fallback, and autonomous delivery commit
proof_policy: seam-test-first
contract_revision: 14
depends_on:
- zero-arg-transition-engine-and-command-runner
- upfront-contract-synthesis-and-shared-helper-extraction
scope:
- src/cli/task.rs
- src/cli/guide.rs
- src/mcp/tools.rs
- src/engine/milestones/contract.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/integration.rs
- tests/acdd/scope_sync.rs
- tests/acdd/tasks.rs
- tests/mcp/tool_evolution/mcp_boundaries.rs
- tests/mcp/freshness/stdio_and_response.rs
- tests/mcp/context.rs
- tests/acdd/persistence.rs
- tests/acdd/milestones.rs
- tests/acdd/mcp.rs
- src/db/tasks_store/recovery.rs
- src/engine/tasks/mod.rs
- docs/reference/profiles.md
- docs/reference/tasks.md
- docs/reference/cli.md
- skills/contextunity-forge/SKILL.md
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/README.md
- docs/future/milestones/AGENTS.md
- docs/milestones/AGENTS.md
- docs/milestones/README.md
- docs/future/references/acdd_profile.v2.yaml.example
- docs/future/AGENTS.md
- src/db/tasks_store/lifecycle.rs
- src/engine/tasks/lifecycle.rs
- src/core/tasks/gates.rs
- src/engine/tasks/gates.rs
- src/db/tasks_store/trace.rs
- src/db/tasks_store/mod.rs
- docs/reference/mcp-tools.md
- docs/milestones/AC-001-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/reference/profiles.md
- src/core/tasks/profile.rs
- src/engine/tasks/context.rs
- .forge/acdd/profile.yaml
- tests/acdd/subtasks.rs
- tests/mcp/skill_sync.rs
- src/cli/milestone.rs
- src/engine/milestones/mod.rs
scope_roots:
- src/cli/task.rs
- src/cli/guide.rs
- src/mcp/tools.rs
- src/engine/milestones/contract.rs
- src/engine/milestones/handoff.rs
- src/engine/milestones/integration.rs
- tests/acdd/scope_sync.rs
- docs/reference/tasks.md
- docs/reference/cli.md
- skills/contextunity-forge/SKILL.md
- docs/future/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md
- docs/adr/README.md
- docs/future/milestones/AGENTS.md
- docs/milestones/AGENTS.md
- docs/milestones/README.md
- docs/future/references/acdd_profile.v2.yaml.example
- docs/future/AGENTS.md
- src/db/tasks_store/lifecycle.rs
- src/engine/tasks/lifecycle.rs
- src/core/tasks/gates.rs
- src/engine/tasks/gates.rs
- src/db/tasks_store/trace.rs
- src/db/tasks_store/mod.rs
- docs/reference/mcp-tools.md
- docs/milestones/AC-001-hierarchical-acdd-gates-and-milestone-orchestration.md
- src/core/tasks/profile.rs
- src/engine/tasks/context.rs
- .forge/acdd/profile.yaml
- docs/reference/profiles.md
- tests/acdd/
- tests/mcp/
- src/engine/tasks/mod.rs
- src/db/tasks_store/recovery.rs
- src/engine/milestones/
- src/cli/milestone.rs
- src/core/tasks/acdd.default.yaml
status: completed
subtasks:
- subtask_ref: extend-scope-markdown-manifest-sync
  title: Implement atomic two-tier scope extension synchronizing SQLite and markdown manifest
  status: completed
  evidence: 'Source path verified: TasksStore::extend_scope -> extend_scope_with_pre_commit; atomically updates manifest with rollback closure around SQLite commit. ACDD tests scope_extension_updates_sqlite_and_milestone_manifest and scope_extension_rejects_invalid_manifest_dependencies_before_persisting passed within `cargo test --test acdd` (132 passed, 0 failed, exit 0); strict Clippy exit 0.'
- subtask_ref: extended-scope-review-proof-enforcement
  title: Implement review gate validator enforcing extended_scope_review sign-off on scope additions
  status: completed
  evidence: 'cargo test --test acdd review_requires_exact_extended_scope_approval_before_delivery: exit 0, 1 passed. Existing seam now rejects missing proof, wrong/extra/duplicate paths, and reason_verified=false; exact approved_paths with reason_verified=true advances to deliver. Forge code_map_explain resolves test calls into src/engine/tasks/gates.rs::TasksStore::submit and src/cli/task.rs::run. Full strict Clippy and ACDD will be rerun before parent build submission.'
- subtask_ref: session-checkpoint-sqlite-migration
  title: Migrate session_checkpoint MCP tool to query gate_checkpoints SQLite table by breadcrumb
  status: completed
  evidence: 'cargo test --test acdd session_checkpoints_use_sqlite_breadcrumb_rows: exit 0, 1 passed; cargo test --test mcp repair_checkpoint_page_shapes_are_opaque_and_persistence_is_unchanged: exit 0, 1 passed; rustfmt --edition 2021 --check src/mcp/tools.rs tests/acdd/scope_sync.rs: exit 0. MCP and ACDD seams both call cli::guide::checkpoint; after `contextunity-forge-mcp scan`, Forge graph shows both edges with matched freshness. Test covers SQLite breadcrumb save/get/list/delete, project namespace isolation, no legacy .forge/checkpoints.json, opaque payload and MCP response behavior. Full strict Clippy and ACDD will be rerun before parent build submission.'
- subtask_ref: milestone-handoff-trace-and-checkpoint-pruning
  title: Extend milestone handoff pruning to purge gate_transitions, traces, and checkpoints tables
  status: completed
  evidence: '`cargo test --test acdd milestone_handoff_prunes_only_its_gate_telemetry_identity`: exit 0, 1 passed. The fixture creates a checkpoint, execution trace, and transition for both m_test and m-test, calls TasksStore::handoff_milestone, and proves only the exact m_test identity is pruned. Forge code_map_explain confirms tests/acdd/milestones.rs -> TasksStore::handoff_milestone -> handoff_milestone_with_unlink -> prune_milestone_gate_records.'
- subtask_ref: milestone-verification-container-and-autonomous-deliver
  title: Implement milestone verification container gate with full repo check, review, fallback subgraph, and autonomous delivery
  status: completed
  evidence: 'Targeted rustfmt check passed. `cargo clippy --all-targets --all-features -- -D warnings`: exit 0. `cargo test --test acdd`: exit 0, 132 passed, 0 failed (697.67s).'
- subtask_ref: promote-future-adr-and-milestone-governance
  title: Promote docs/future/adr/0020-*.md to docs/adr/ and docs/future/milestones/AGENTS.md to docs/milestones/AGENTS.md, registering ADR 0020 in docs/adr/README.md and pruning docs/future/
  status: completed
  evidence: Verified already satisfied in current worktree; no edits needed. docs/adr/0020-hierarchical-acdd-gates-and-milestone-orchestration.md exists with accepted status and Forge get_doc retrieves it; docs/adr/README.md lists ADR 0020 as item 20; docs/milestones/AGENTS.md contains promoted governance rules; docs/future/adr/ and docs/future/milestones/ are absent. docs/future/AGENTS.md states accepted architecture and milestone rules were promoted and retains only the v2 profile example routed to Task 1. Forge scan completed and document retrieval is current.
- subtask_ref: zero-argument-task-submit-cli-wiring
  title: Wire the CLI task submit command to Task 3's zero-argument submission engine
  status: completed
  evidence: cargo test --test acdd task_submit_cli_accepts_omitted_evidence_and_uses_automatic_proof — exit 0, 1 passed. CLI parses omitted --evidence and dispatches to the claim-bound automatic proof engine.
- subtask_ref: subtask-completion-delivery-guard
  title: Implement mechanical delivery blocker enforcing 100% terminal subtask completion and evidence verification
  status: completed
  evidence: '`cargo test --test acdd delivery_requires_terminal_subtasks_with_verification_evidence`: exit 0, 1 passed. Cases verify incomplete subtasks and completed subtasks with missing or whitespace evidence block delivery without advancing task state. Forge code_map_explain confirms the test calls engine::tasks::submit.'
- subtask_ref: profile-clean-room-inheritance-control
  title: Implement inherit none clean-room profile isolation without embedded catalog leakage
  status: completed
  evidence: '`cargo test --test acdd declarative_profile_compiles_defaults_and_respects_role_overrides`: exit 0, 1 passed. The existing profile seam verifies inherit:none clears embedded roles, commands, error codes, proofs, guidance, policies, contours, and templates; Forge code_map_explain confirms calls to load_for_workspace and validate_profile_document.'
- subtask_ref: profile-universal-commands-catalog-fix
  title: Support empty commands catalog in compiler and eliminate hardcoded language dispatch from context selector
  status: completed
  evidence: '`cargo test --test acdd declarative_profile_compiles_defaults_and_respects_role_overrides` and `cargo test --test acdd boundary_guards_follow_configured_proof_kinds_and_non_cargo_commands_keep_paths`: each exit 0, 1 passed. The profile seam proves explicit commands:{} clears the inherited registry when command gates are absent; the boundary seam proves all configured non-Cargo commands keep the ordinary scoped path.'
receipt:
  contract_revision: 14
  passed_at: 2026-10-25T09:49:48.629563663+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 133
      tests_failed: 0
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Reviewed immutable candidate 8a9d7166884bef8747dadc304c7a6a4f70a54104 against captured base 1a000bc6d1a9774854fb7e93f36b659166b2f382 using the full explicit effective scope. Its only delta from reviewed 566c92e4 is ADR0020 line 353 link correction plus trailing whitespace cleanup. All 41 surviving snapshot paths and 3 admitted deletions remain authorized; protected .forge/acdd/profile.yaml was initially admitted. command_gate.rs is explicitly in Forge allowed_scope and authorized beneath manifest scope_roots src/engine/milestones/. I approve that extension as necessary for the full-log caller at integration.rs:268. Scoped bytes match worktree, including untracked promoted ADR blob 725c7769f8b619bd225279e37c5a3e3df667f95f. Unrelated dirty tests/AGENTS.md and sibling source/test paths remain excluded and untouched.
        claims:
          applicable: true
          evidence: 'All T6-F1–F12 are closed. F12 corrected ../../src link resolves to src/engine/milestones/integration.rs; matched Forge graph now verifies ADR0020:353 -> run_milestone_check:204. F1–F11 source/tests are byte-identical to 566c92e4 independently reviewed in #1230: optional CLI evidence reaches submit_zero_arg; exact extended_scope_review required; scope synchronization uses managed publication; SQLite breadcrumbs preserve opaque payloads and protect lifecycle ownership/provenance; full log persistence includes bounded trace snippets; Delivered source/executable-skill drift requires fresh verify/review; admitted persist_log has pub(super); Exhausted action matches persisted writer and retries stay terminal. Existing three production CLI milestone verification seams independently passed on identical source/tests (exit 0, 3 passed, 14.40s). Fresh accepted builder strict Clippy plus ACDD proof in #1231 is exit 0, 133 passed, 0 failed. No new behavior or test change requires another runtime rerun.'
        concurrency:
          applicable: true
          evidence: Revalidated unchanged managed transaction/revision/root/conflict fences and preimage-checked recoverable manifest publication. Existing rollback, blocked-writer and replay seam coverage remains unchanged. Milestone verification retains prepared store lock, candidate/binding/HEAD checks, clean-tree validation, bounded two-attempt fallback and claim-bound repair review. Exhausted reload/retries reject without additional command/history. Generic scratch checkpoint SQL upsert/delete predicates cannot replace transition-owned state; lifecycle loader verifies identity, tree, phase/action and proof digest. Candidate immutability preserved throughout review.
        project_isolation:
          applicable: true
          evidence: Unchanged namespace-confined breadcrumb save/get/list/delete and exact milestone transition lookup prevent cross-project access. Handoff deletes checkpoint/trace/transition rows by fully qualified equality, preserving neighbors. Scope extensions remain normalized, protected-path checked and confined to admitted roots. Full logs use hashed identifiers, canonical worktree root, no-follow directory traversal and exclusive files. Reviewer codex-ac001-task6-reviewer-20261025-formal-r5 differs from accepted builder codex-ac001-task6-builder-20261025. Every task operation uses the claimed worktree; no sibling edits.
        administration:
          applicable: true
          evidence: F12 closes the sole unresolved Markdown target from the prior full scoped link check; all other Markdown links are byte-unchanged. Forge now returns the ADR section as verified documentation for run_milestone_check, with matched freshness on 387 files and 0 parse errors. Scoped diff --check passes. Promoted ADR/governance, updated CLI/MCP/reference behavior, clean-room/empty-command profiles, terminal subtask evidence guard, telemetry pruning and durable archive/tombstone authority remain as reviewed. No receipt/schema/Git mutations and no delivery performed. All twelve prior findings closed; no remaining scoped findings.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 3)
- `fn:extend_scope_dual_sync`
- `fn:verify_extended_scope_review`
- `fn:purge_milestone_handoff_traces`
- `fn:execute_milestone_verification_container`
- `cmd:task submit`
- `cmd:task extend-scope`
- `cmd:milestone handoff`

---

### task: acdd-test-suite-modularization-and-domain-decomposition

```yaml
task_ref: acdd-test-suite-modularization-and-domain-decomposition
target: Decompose monolithic 10k-line tests/acdd/tasks.rs into domain-partitioned modules under tests/acdd/tasks/, extract common workspace fixtures into tests/acdd/support.rs, establish multi-tier AGENTS.md instruction hierarchy across tests/ and tests/acdd/, and update test suite documentation
proof_policy: direct-proof
contract_revision: 2
depends_on:
- container-gate-and-claim-engine
- scope-extension-manifest-sync-and-review-audit
scope:
- tests/acdd/tasks.rs
- tests/acdd/tasks/mod.rs
- tests/acdd/tasks/lifecycle.rs
- tests/acdd/tasks/seams.rs
- tests/acdd/tasks/boundaries.rs
- tests/acdd/tasks/workspaces.rs
- tests/acdd/tasks/guidance.rs
- tests/acdd/tasks/retirement.rs
- tests/acdd/tasks/AGENTS.md
- tests/acdd/mod.rs
- tests/acdd/support.rs
- tests/acdd/AGENTS.md
- tests/AGENTS.md
- docs/testing/README.md
scope_roots:
- tests/
- tests/acdd/
- docs/testing/
- src/engine/tasks/
status: completed
subtasks:
- subtask_ref: create-acdd-instruction-hierarchy
  title: Author tests/acdd/AGENTS.md and tests/acdd/tasks/AGENTS.md formalizing modular test boundaries, file size ceilings, and harness reuse
  status: completed
  evidence: Added tests/acdd/AGENTS.md and tests/acdd/tasks/AGENTS.md with module ownership, routing, shared-fixture guidance, and a 4,000-line ceiling; docs links and rustfmt checks passed.
- subtask_ref: extract-acdd-shared-fixtures
  title: Extract shared workspace setup helpers and fixtures into tests/acdd/support.rs
  status: completed
  evidence: 'Moved eight common indexed-boundary/workspace fixture helpers into tests/acdd/support.rs. The full ACDD suite passed: 133 passed, 0 failed; strict Clippy passed.'
- subtask_ref: partition-acdd-tasks-domain-modules
  title: Decompose tests/acdd/tasks.rs into domain modules under tests/acdd/tasks/ preserving all 116 tests without regressions
  status: completed
  evidence: Replaced monolithic tests/acdd/tasks.rs with six domain modules under tests/acdd/tasks/. Preserved all 133 collected ACDD tests, including all 66 task tests; full suite passed.
- subtask_ref: update-root-test-router-and-docs
  title: Update tests/AGENTS.md and docs/testing/README.md with new domain module routes and file size invariants
  status: completed
  evidence: Updated tests/AGENTS.md and docs/testing/README.md to route the ACDD task suite and describe module boundaries/ceiling. Relative-link check and Forge documentation retrieval passed.
receipt:
  contract_revision: 2
  passed_at: 2026-10-25T10:49:23.762694767+00:00
  evidence:
    command_proof:
      command: cargo clippy --all-targets --all-features -- -D warnings && cargo test --test acdd
      exit_code: 0
      tests_passed: 133
      tests_failed: 0
      log: 'Strict Clippy passed. ACDD: 133 passed, 0 failed, finished in 377.41s.'
  review:
    review_proof:
      decision: pass
      contours:
        paths:
          applicable: true
          evidence: Inspected candidate a764e23 as a scope-projected tree; all 14 included paths are admitted, including the explicit src/engine/tasks/mod.rs extension. Hashes of every candidate path match the current worktree, so there are no post-snapshot edits. Other dirty files, including tests/acdd/runner.rs, are outside the candidate and are not part of this task snapshot.
        claims:
          applicable: true
          evidence: The six domain modules are registered under tests/acdd/tasks/mod.rs, shared workspace helpers are in tests/acdd/support.rs, and the new instructions/docs route those owners. The seam test delivery_requires_terminal_subtasks_with_verification_evidence directly calls engine::tasks::submit twice and asserts the delivery error plus unchanged persisted task state. Forge reports a fresh exact call edge, no parse errors, and zero untested public surface; the accepted build proof reports strict Clippy and 133 ACDD tests passing.
        concurrency:
          applicable: true
          evidence: 'No production logic changed: src/engine/tasks/mod.rs is identical to candidate baseline 8a36c361. The refactor retains the lifecycle lock/mutation test and scoped snapshot/worktree concurrency coverage in their domains; all 133 ACDD tests pass. Forge task_list found no other active task owner in this milestone.'
        project_isolation:
          applicable: true
          evidence: The refactor preserves ScopedWorkspace-based temporary roots and task-local SQLite fixtures. Workspace isolation and linked-scope tests remain in the workspaces domain. No repository, project identity, production path, or database behavior was modified.
        administration:
          applicable: true
          evidence: The candidate changes tests and testing guidance only, with the admitted unchanged engine module included as seam context. No schema, migration, profile, configuration, receipt, or protected milestone block changed. Candidate hashes match the reviewed tree; the recorded build passed strict Clippy and the full affected ACDD target.
  decision: pass
  rollup:
    verified_invariants:
    - 'INV-UNIVERSAL-GATE-POLYMORPHISM: Every workflow node is a Gate; execution mode is determined by driver fields: Agent Gate (role), Command Gate (command), or Container Gate (gates).'
    - 'INV-ZERO-ARGUMENT-SUBMISSION: Automated gate submissions (any gate configured with proof: command, contract, or delivery) require zero manual JSON arguments; Forge autonomously compiles proof from gate execution traces and Git tree state. Cognitive review gates (proof: review) strictly require typed review judgments (verdict, reviewer, contours_reviewed, extended_scope_review).'
    - 'INV-IN-PLACE-LIFECYCLE: Task and milestone execution supports in-place operation (--in-place / worktree: ".") without mandatory worktree or branch isolation.'
    - 'INV-SILENT-ENGINE-TELEMETRY: Gate transitions and command execution traces are automatically recorded in SQLite; automated engine progress logging to task_blackboard is suppressed, preserving blackboard bandwidth for RFCs, synthesized contract matrices, and human escalations.'
    - 'INV-SEAM-ENGINE-RED-GREEN: Seam Engine automatically tracks contract runtime failure (matching configured expected_failure.exit_codes) to implementation pass (exit 0) via gate execution traces.'
    - 'INV-SEAM-REACHABILITY-RESILIENCE: Static seam verification proves the current test-to-production connection through imports, re-exports, and bounded test-helper paths; stale source/index evidence produces an actionable stale-index diagnostic. An unresolved expression alone does not prove reachability.'
    - 'INV-REQUIRED-SKILLS-GOVERNANCE: Manifests declare required_skills in frontmatter; milestone init fails closed (MILESTONE_PREFLIGHT_FAILED) before agent launch if any declared skill is absent.'
    - 'INV-PREFLIGHT-MILESTONE-INIT: milestone init validates manifest syntax, checks required_skills on host, and pre-scaffolds base worktrees across declared repositories upfront before agent dispatch.'
    - 'INV-MULTI-REPO-MANIFEST-DECLARATION: Multi-repository workflows declare participant repositories in manifest frontmatter; the engine coordinates cross-repo worktree initialization and compound breadcrumbs.'
    - 'INV-EXTENDED-SCOPE-MANIFEST-SYNC: task extend-scope writes atomically to both SQLite and the milestone markdown manifest; review proof strictly requires extended_scope_review approval before deliver.'
    - 'INV-BREADCRUMB-CHECKPOINT-UNIFICATION: Checkpoints are stored in SQLite gate_checkpoints keyed directly by turn breadcrumb locators; legacy .forge/checkpoints.json is eliminated.'
    - 'INV-TRANSITION-TRACE-PERSISTENCE: Upon milestone handoff (INV-HANDOFF-SQLITE-PRUNING), completed milestone tasks, claims, revisions, gate transitions, execution traces, checkpoints, and task blackboard rows are purged from SQLite; milestone_tombstones is retained to keep archived dependencies satisfiable; canonical durable records reside permanently in docs/milestones/archive/*.md and Git history.'
    - 'INV-PRE-DOC-AUTONOMOUS-VERIFICATION: Milestone verification executes as a container gate with automated checks, review, fallback repair subgraph, and autonomous delivery before documentation.'
    - 'INV-MARKDOWN-FIRST-ERGONOMICS: Agent messages, guidance, and status reports follow Milestone 054 Markdown-first principles: clickable coordinates, callout blockers, compact status tables, zero dead metadata.'
    - 'INV-MODULAR-SUBSYSTEM-DECOUPLING: Forge MCP decomposes functionality into 5 independent subsystems (codemap, acdd, blackboard, docs, checkpoints) configurable via forge-mcp.yaml modules/profile, with dynamic MCP tool registration and zero initialization cost for disabled subsystems.'
    - 'INV-CONTEXT-AWARE-GUIDANCE-ADAPTATION: workflow_guidance, prompts, and gate verification dynamically adapt to enabled subsystems, suppressing dead tool references, omitting unused guidance structures, and falling back gracefully when graph or blackboard features are disabled.'
    - 'INV-PAYLOAD-OVERFLOW-GUARD: Custom metadata, git_commit_note payloads (rendered as Markdown by default, or JSON), and in-database trace snippets attached to tasks are bounded by a 2 KiB limit (configurable via git_commit_note.limits.max_bytes / limits.max_metadata_bytes) with complete command outputs written to disk logs, protecting the 96 KiB MCP response budget and preventing Git note bloat.'
    - 'INV-UNIVERSAL-PROFILE-ZERO-LANGUAGE-HARDCODE: The embedded default profile (acdd.default.yaml) and profile templates must remain strictly language-agnostic. Gates and commands must not hardcode Rust-specific binaries (such as cargo clippy or cargo test); all verification commands must use pattern-based dispatch (by_pattern) matching project file extensions or generic command identifiers. Guidance texts must not mention specific languages or compilers.'
    - 'INV-TWO-LOOP-ACDD-GOVERNANCE: Task execution strictly distinguishes between scope: (files on disk, extensible by builder) and symbols: (admitted public interface deltas, strictly governed by architect). Review failures route via reject_routing (scope -> build, symbols -> contract). Unadmitted public interface mutations are prohibited from bypassing the Long Outer Loop (architect arbitration -> seam contract test -> builder).'
    - 'INV-GUIDANCE-SLOT-BINDING: Gates declare parameterized guidance slots (e.g. guidance: [task.contract, hitl]), and milestones bind them in manifest frontmatter (guidance: { hitl: pro }). The engine resolves slot hitl with value pro via format!("{}.{}", slot, value) -> hitl.pro (or literal dotted keys). The validator verifies slot matching on milestone init/check, emitting warnings for unbound slots and falling back to profile defaults.'
    architectural_notes: []
    review_summary:
      decision: pass
      contours:
        administration: accepted
        claims: accepted
        concurrency: accepted
        paths: accepted
        project_isolation: accepted
```

#### Admitted Symbols (Wave 5)
- `mod:tests::acdd::tasks`
- `mod:tests::acdd::tasks::lifecycle`
- `mod:tests::acdd::tasks::seams`
- `mod:tests::acdd::tasks::boundaries`
- `mod:tests::acdd::tasks::workspaces`
- `mod:tests::acdd::tasks::guidance`
- `mod:tests::acdd::tasks::retirement`

