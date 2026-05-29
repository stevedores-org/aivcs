# GitHub Copilot Instructions — aivcs

- **Project Type**: Rust Workspace (Version Control System).
- **Core Stack**: Rust, Tokio, Reqwest, Serde.
- **Conventions**:
  - Prefer async/await via Tokio.
  - Follow TDD: write tests for logic in `tests/` or `#[cfg(test)]` modules.
  - Adhere to content-addressed data patterns.
- **Style**:
  - Idiomatic Rust 2021.
  - No unsafe blocks without `// SAFETY:` comments.
  - Clean clippy output is mandatory.
