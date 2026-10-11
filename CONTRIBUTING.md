# Contributing to ContextUnity Forge MCP

Thank you for your interest in contributing to **ContextUnity Forge MCP**! We welcome community contributions, bug reports, documentation enhancements, and feature proposals.

---

## Getting Started

### Prerequisites

To build and test Forge locally, ensure you have:
- **Rust Toolchain**: Stable Rust (edition 2021). Install via [rustup.rs](https://rustup.rs).
- **C Compiler**: `gcc` or `clang` (required for compiling Tree-sitter parsers and bundled SQLite).
- **Git**

### Building the Project

Clone the repository and build the binary:

```bash
git clone https://github.com/ContextUnity/contextunity-forge-mcp.git
cd contextunity-forge-mcp

# Build default profiles (Python, Rust, TS/JS, Vue, HTML, YAML, TOML, Protobuf)
cargo build --locked

# Build with all extended language profiles (Go, Java, C#, Kotlin, PHP, Ruby, C/C++)
cargo build --locked --features all-languages
```

---

## Quality and Verification Standards

Forge enforces strict code quality and verification gates on every contribution.

Before submitting a Pull Request, ensure all checks pass:

1. **Format Check**:
   ```bash
   cargo fmt --check
   ```

2. **Strict Clippy (Zero Warnings)**:
   ```bash
   cargo clippy --all-targets -- -D warnings
   ```

3. **Test Suite**:
   ```bash
   cargo test --locked
   ```

4. **Required Gates**:
   Refer to [TESTS.md](TESTS.md) for domain-specific test counts, milestone handoff gates, and verification rules.

---

## Contribution Workflow

### 1. Issues & Discussions
- If you find a bug, open an issue detailing steps to reproduce, client environment (e.g. VS Code, Cursor, Claude Desktop), and relevant error logs.
- For architectural proposals or adding new language profiles, start a discussion or draft an issue first.

### 2. Creating a Branch
- Branch from `main` with a descriptive name (`fix/issue-description` or `feat/feature-name`).
- For milestone work managed via ACDD, follow the worktree guidelines in [AGENTS.md](AGENTS.md).

### 3. Writing Tests First (Seam & Regression Tests)
- Every bug fix should include a failing regression test proving the defect before the fix.
- New capabilities should include production-seam tests exercising observable behavior rather than internal mock-only details.

### 4. Documentation Co-Evolution
- If your change affects public CLI commands, MCP tool contracts, configuration schemas, or workflows:
  - Update corresponding documentation under `docs/reference/`.
  - Preserve documentation formatting rules specified in [docs/AGENTS.md](docs/AGENTS.md) (frontmatter, backticked code symbols, invariant alerts).

### 5. Submitting a Pull Request
- Provide a clear, concise PR description summarizing the changes and referencing any related issues.
- Verify that CI passes all format, clippy, and test gates.

---

## Community Guidelines

- Be respectful, constructive, and collaborative in discussions, code reviews, and issue threads.
- Adhere to open-source best practices.

---

## License

By contributing to ContextUnity Forge MCP, you agree that your contributions will be licensed under its dual [MIT](LICENSE-MIT) and [Apache-2.0](LICENSE-APACHE) licenses.
