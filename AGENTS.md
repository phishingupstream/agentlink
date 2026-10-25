# agentlink agent guidance

## Purpose

`agentlink` is a small Unix Rust CLI that keeps a remote Git registry available offline and links its instructions and Agent Skills into coding-agent paths.

## Constraints

- Use the installed `git` binary. Do not add `libgit2`, `gix`, or custom transport and merge logic.
- Check for updates from agent session-start hooks. Do not add a daemon, async runtime, watcher, or scheduler.
- Keep user configuration to the registry URL, configured agents, selected user skills, and tracked project paths.
- Keep registry paths fixed at optional `global/AGENTS.md` and required `skills/<name>/SKILL.md`.
- Preserve skill contents. Do not parse, translate, or copy provider-specific variants.
- Treat generated instruction and skill destinations as agentlink-owned and replace collisions directly.
- Link project skills into `.claude/skills` for Claude and `.agents/skills` for Codex, Gemini, and Pi; remove agentlink-owned links from the retired `.gemini/skills`, `.pi/skills`, and `.opencode/skills` paths.
- Keep project selection in the committed `.agentlink.toml` manifest.
- Support Linux and Unix symbolic links. Do not add Windows abstractions without a concrete requirement.

## Architecture

- `src/main.rs`: CLI, Git subprocesses, TOML models, reconciliation, and unit tests.
- [REQUIREMENTS.md](REQUIREMENTS.md): approved behavior and acceptance boundaries.
- `integrations/`: bundled Claude, Codex, Gemini, and Pi session-start integrations.
- `tests/integration.rs` and `tests/integration/`: compiled integration suite and optional Kubernetes launcher.
- `tests/common/mod.rs`: isolated filesystem/Git fixtures and hook lifecycle checks.
- `~/.config/agentlink/config.toml`: machine-local registry URL, enabled agents, user skills and tracked projects.
- `~/.local/share/agentlink/registry`: offline Git checkout.
- `<project>/.agentlink.toml`: committed project skill selection.

## Verification

Run the formatting, lint, tests and fresh-install checks in [README.md](README.md#develop-and-test). Test the full provider suite in the integration image; default Cargo tests intentionally skip tests requiring provider CLIs. Fixtures must not use host agent configuration or credentials. Preserve latest CLI installation at test startup, version logs, exact image digests, execution deadlines and cleanup for Kubernetes runs.

Use release builds for size and performance measurements. Profile before micro-optimizing; the `profiling` Cargo profile retains symbols. Public releases contain the tested source commit, license, supported-platform binary and checksums.
