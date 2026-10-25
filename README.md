# AgentLink

AgentLink syncs your skills across agents and helps them maintain the library.

Git and symlinks, as it should be. Skills are dependencies. Keep them upstream, let projects consume them, and let agents contribute fixes back. AgentLink supplies the harness integrations and guidance that make this work.

## Install

Download the [Linux x86-64 release](https://github.com/phishingupstream/agentlink/releases/tag/v0.6.0), then install the extracted binary:

```sh
tar -xzf agentlink-0.6.0-linux-x86_64.tar.gz &&
mkdir -p "$HOME/.local/bin" &&
install -m 755 agentlink-0.6.0-linux-x86_64/agentlink "$HOME/.local/bin/agentlink"
```

Add `~/.local/bin` to `PATH`. AgentLink needs Git and your usual Git credentials; SSH remotes also need an SSH client. Linux x86-64 is tested; macOS is unverified and Windows is unsupported. [Build from source](docs/development.md#install-from-source) with Rust 1.99 or newer.

## Get started

Use a Git repository containing your skills:

```text
agent-config/
├── global/AGENTS.md          # optional global instructions
└── skills/
    └── release/
        └── SKILL.md
```

Set `AGENTLINK_REGISTRY` to its clone URL or local path. Initialize once, selecting your installed agents: `claude`, `codex`, `gemini` or `pi`. This example uses Claude and Codex; their CLIs must be on `PATH` to install the hooks.

**Move existing instructions or same-named skills you want to keep before setup:** AgentLink replaces its managed destinations. Unrelated skills stay in place.

```sh
agentlink init "$AGENTLINK_REGISTRY" --agents claude,codex &&
agentlink install-hooks
```

From a project directory:

```sh
agentlink use release &&
agentlink status
```

Commit `.agentlink.toml`; ignore the generated skill links. The project now uses `release` from the registry. Session hooks check for updates at most once per 15 minutes; run `agentlink update` for an immediate refresh. Linking and lookup work offline after initialization.

[Maintain the library](docs/usage.md#maintain-the-library) explains how you and your agents edit, review and publish skills. The [usage guide](docs/usage.md) covers a local trial, user-level skills, migration and configuration. See [development](docs/development.md) for tests and container builds, or `agentlink --help` for commands.

## License

[CC BY-NC 4.0](LICENSE) — free for non-commercial use with attribution. Commercial use requires a separate paid license from the copyright holder. Third-party dependencies retain their own licenses.
