# Using AgentLink

For installation and first setup, see the [README](../README.md). Run `agentlink --help` or `agentlink <command> --help` for command syntax.

## Try a local registry

This example needs AgentLink and Git, but no agent CLI or remote repository. Run it in a fresh directory on an account where AgentLink has not been initialized. It writes the normal user configuration and creates a project skill link; `init` refuses an existing configuration or registry checkout.

```sh
(
    set -eu
    mkdir -p registry/skills/example project
    printf '%s\n' '---' 'name: example' 'description: An example shared skill' '---' '# Example' > registry/skills/example/SKILL.md
    git -C registry init
    git -C registry add .
    git -C registry -c user.name=Example -c user.email=example@example.invalid commit -m 'Add example skill'
    agentlink init ./registry --agents codex
    cd project
    agentlink use example
    agentlink status
    agentlink search example
)
```

The project contains `.agentlink.toml` and a `.agents/skills/example` symlink. Relative local registry paths are saved as absolute paths, so updates work from other directories. For an isolated installation test that leaves your configuration alone, use [the source installation check](development.md#run-the-checks).

## Select skills

Before selecting a skill, move any existing same-named file or directory at its destination that you want to keep. AgentLink replaces that destination; unrelated unmanaged skills are preserved.

From a project directory, select skills that exist in your registry:

```sh
agentlink use release
```

`use` records the project in your user configuration and writes its skill selection to `.agentlink.toml`. Commit the manifest; ignore generated links under `.claude/skills` and `.agents/skills`. Claude uses the former; Codex, Gemini and Pi use the latter.

To select a skill for every project, use the independent user-level selection:

```sh
agentlink use release --user
```

User-level links use the same agent directories under your home directory. Remove selections with `agentlink unuse release` or `agentlink unuse release --user`.

`agentlink untrack` removes a project's managed links and stops tracking it, but keeps its `.agentlink.toml`. Removing the last project skill also untracks the project.

## Maintain the library

AgentLink's working checkout is `~/.local/share/agentlink/registry`. Edit the source skill there; project links point to the same files. Fetch upstream changes before starting:

```sh
agentlink update &&
git -C "$HOME/.local/share/agentlink/registry" status --short
```

Give an agent that checkout path and the skill to change. Put your review, verification and publication rules in the registry's own `AGENTS.md`; use `global/AGENTS.md` for instructions you want linked into every enabled agent. Tell agents to edit the registry source and follow those rules when contributing changes.

After editing and checking a `release` skill, review its diff:

```sh
git -C "$HOME/.local/share/agentlink/registry" diff -- skills/release
```

Then publish the intended files with your normal Git workflow:

```sh
(
    set -eu
    cd "$HOME/.local/share/agentlink/registry"
    git add skills/release
    git commit -m 'Update release procedure' -- skills/release
    git push
    agentlink sync
)
```

Edits are immediately visible through local links. Other machines receive committed, pushed changes on their next update. `sync` reconciles links without fetching; `update` fetches and fast-forwards the registry before reconciling every tracked project.

If Git cannot fast-forward, inspect the checkout with `git status` and `git log --oneline --graph --all`, then resolve the local changes or diverged history using Git. AgentLink does not merge or discard your work.

## Move existing skills into the registry

Find unmanaged project-local skills:

```sh
agentlink discover ~/projects
```

Discovery checks that directory and its immediate child projects, including current and retired agent skill paths. It does not recursively scan your repository trees.

To move a discovered skill, use its name and project path:

```sh
agentlink migrate example --project ~/projects/my-app
```

Migration requires a clean registry checkout and a skill name not already in the registry. It fast-forwards the checkout, copies the complete skill directory unchanged, commits and pushes it, then selects the skill and replaces the project-local copies with links. The commit records your Git identity and the source repository's `origin` URL, or its local path if it has no origin.

If the name already exists, use `agentlink use example` to select it, or rename the local skill before migration. When multiple agent paths contain the same local skill, choose the source with `--source <path>`.

A failed push leaves the project-local skill intact and the migration commit in the registry checkout. Inspect or push that commit before proceeding; source copies are removed only after publication succeeds.

## Inspect and update

Run `agentlink` or `agentlink status` inside a project to see user and project selections and whether their links are current. An unenabled project gets a suggested `agentlink use` command.

```sh
agentlink search release
```

Search uses cached contents, matches whole words without case sensitivity, and ranks matching skill names first, followed by match count. It returns up to ten results; change that with `--limit`. Redirected output and `NO_COLOR` disable terminal styling.

`agentlink update` fetches the registry and reconciles user selections and every tracked project. When a selected skill has been removed upstream, it removes the corresponding managed links and manifest entry and warns with the affected project path. Review and commit changed manifests. Projects with no remaining selected skills are untracked; unmanaged skills are preserved.

## Session hooks

`agentlink install-hooks` installs integrations for the agents selected in your configuration. Claude and Codex register the bundled plugin through their CLIs; Gemini and Pi use local integration links. AgentLink must remain on the agents' `PATH`.

The hooks call `agentlink update-if-due`, sharing a 15-minute check interval across agents. Update failures warn without blocking session startup. Run `agentlink update` to reproduce an error immediately; check Git credentials, connectivity and the registry checkout as appropriate.

Run `agentlink install-hooks` again after changing enabled agents or upgrading AgentLink. It installs enabled hooks and removes disabled ones. `agentlink uninstall-hooks` removes all AgentLink session hooks and generated integration assets.

OpenCode is no longer supported. Synchronization removes only AgentLink-owned retired OpenCode instruction, hook and skill links, plus old `.gemini/skills` and `.pi/skills` links. Unmanaged files remain intact.

## Configuration

Machine-specific state lives in `~/.config/agentlink/config.toml`:

```toml
registry = "ssh://git@example.com/team/agent-config.git"
agents = ["claude", "codex"]
skills = ["release"]
projects = ["/home/you/projects/my-app"]
```

Use your own registry URL and project paths. AgentLink uses installed Git and your existing Git credentials. HTTPS remotes need CA certificates; SSH remotes need an SSH client.

The registry must contain `skills/<name>/SKILL.md`. Optional `global/AGENTS.md` supplies user-level instructions; adding or removing it installs or removes those links on the next sync. Instruction destinations are managed in the same way as skill destinations, so preserve any existing content before enabling them.

The registry checkout lives at `~/.local/share/agentlink/registry`, generated integration assets at `~/.local/share/agentlink/integrations`, and the shared hook timestamp at `~/.local/share/agentlink/last-update-check`. Project selections belong in the project's committed `.agentlink.toml`; they do not include your registry URL or machine paths.

Run `init` only for first setup. Upgrading or rolling back the executable preserves the user configuration and registry checkout; reinstall hooks afterward to use that binary's bundled integration assets.

If initialization wrote the configuration but failed while linking, fix the reported filesystem problem and run `agentlink sync`. If only the checkout exists, preserve it by moving it aside before retrying initialization.
