# agentlink

Keep one Git-backed agent configuration available offline and link selected Agent Skills for one project or every project.

## Download a release

[GitHub Releases](https://github.com/phishingupstream/agentlink/releases) provide a Linux x86-64 binary requiring glibc 2.36 or newer. Download `agentlink-0.6.0-linux-x86_64.tar.gz` and `SHA256SUMS` from the same release, then:

```sh
sha256sum -c SHA256SUMS
tar -xzf agentlink-0.6.0-linux-x86_64.tar.gz
install -Dm755 agentlink-0.6.0-linux-x86_64/agentlink "$HOME/.local/bin/agentlink"
export PATH="$HOME/.local/bin:$PATH"
agentlink --version
```

Git must be installed for Git sources. The archive includes the product license and third-party notices. For other platforms, build from source; only Linux x86-64 is verified for this release.

## Install

Linux x86-64 is tested. AgentLink uses Unix symbolic links and process APIs; Windows is unsupported and macOS is not currently verified. Building requires Rust/Cargo 1.99 or newer and a C linker. Runtime requires Git on PATH; HTTPS registries need CA certificates, and SSH registries need an SSH client and your Git credentials.

Clone the repository and install:

```sh
git clone https://github.com/phishingupstream/agentlink.git
cd agentlink
cargo install --path . --locked
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
agentlink --version
```

Re-run the installation after checking out an updated release. The binary embeds its hook assets; no other repository files need installing. Your configuration and registry checkout remain in your home directory. To roll back, install from an earlier release. If the command is missing, check PATH; linker errors indicate missing system build tools.

## Try it locally

This example needs only the installed binary and Git. Run it in a fresh directory; it creates a local Git registry and a project:

```sh
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
```

The project now has `.agentlink.toml` and a `.agents/skills/example` symlink. No agent CLI is needed for linking. Initialization writes to your home directory and refuses an existing AgentLink configuration. For an isolated trial, set `HOME` to an empty temporary directory before running the example. Relative local registry paths are saved as absolute paths, so later updates work from other directories.

## Get started

For normal setup, use your own Git registry instead of the local example above (initialize once):

```bash
agentlink init ssh://git@gitlab.example/apps/agent-config.git \
  --agents claude,codex,pi
agentlink install-hooks
```

The registry stores skills at a fixed path and can provide optional global instructions:

```text
agent-config/
├── global/AGENTS.md
└── skills/
    └── rust/
        └── SKILL.md
```

Selecting a skill replaces any existing file or directory at that skill’s managed destination. Global instruction links similarly replace their destination; move content you want to keep before initializing or selecting skills. Unrelated unmanaged skills are preserved.

Agentlink clones it to `~/.local/share/agentlink/registry`. If `global/AGENTS.md` exists, agentlink installs the user-level instruction links; removing the file removes those links during the next sync. After initialization, `sync`, `use`, and `unuse` work from that checkout without network access.

`install-hooks` installs an agentlink-owned session-start integration for each enabled agent. Claude and Codex register the bundled local plugin through their CLIs; Gemini and Pi receive local integration symlinks. Every integration runs `agentlink update-if-due`, which performs at most one update check per 15-minute interval.

Run `agentlink` or `agentlink status` inside a project to check user-level skills and the current project's selected registry skills. If the project is not enabled, the command shows the `agentlink use <skill>` command to start.

## Add skills to a project

```bash
cd ~/projects/my-app
agentlink use rust docker
```

`use` creates or updates `.agentlink.toml`, records the project in the user configuration, and creates skill links in `.claude/skills` for Claude and `.agents/skills` for Codex, Gemini, and Pi. Commit `.agentlink.toml`; ignore the generated skill directories.

To use a skill in every project, install it at user level:

```bash
agentlink use rust --user
agentlink unuse rust --user
```

User-level selections live in `~/.config/agentlink/config.toml`. Agentlink links them into `~/.claude/skills` for Claude and `~/.agents/skills` for Codex, Gemini, and Pi. User and project selections are independent.

OpenCode support has been retired. Existing configurations ignore its old selection; sync removes only AgentLink-owned OpenCode instruction, hook, and legacy skill links. Unmanaged files remain intact.

Remove individual skills or stop tracking the project:

```bash
agentlink unuse rust
agentlink untrack
```

Removing the last selected skill also untracks the project. `untrack` keeps `.agentlink.toml` but removes the project from your user configuration and deletes agentlink-owned skill links.

## Move local skills into the registry

Find skills that individual apps still own:

```bash
agentlink discover ~/projects
```

`discover` checks the directory itself and each immediate child project. It reports unmanaged skills from `.claude/skills`, `.agents/skills`, and the retired provider-specific skill paths without following the rest of each repository tree.

Move one discovered skill into the shared registry:

```bash
agentlink migrate example \
  --project ~/projects/my-app
```

`migrate` requires a clean registry checkout. It fast-forwards the registry, copies the complete skill directory without changing its contents, commits and pushes the new registry skill, adds it to the project's `.agentlink.toml`, and replaces every project-local copy with agentlink links. The migration commit records your Git identity and the source repository's `origin` URL; projects without an `origin` use their local path.

Skill names are unique across the registry. If the name already exists, agentlink stops without overwriting it; use `agentlink use <skill>` to select the existing version or rename the local skill before migrating it. If the same local skill appears in multiple agent paths, select the source with `--source <path>`.

If the registry push fails, the local skill remains in the project and the new registry commit remains in the offline checkout for you to inspect or push. Agentlink removes project-local copies only after the registry push succeeds.

## Inspect and refresh skills

```bash
find ~/.local/share/agentlink/registry/skills -mindepth 1 -maxdepth 1 -type d -printf '%f\n' | sort
agentlink search profiling
agentlink search rust --limit 5
agentlink update
```

Search performs a case-insensitive whole-word search, ranks matching skill names first and otherwise orders by match count, and prints up to ten compact result cards. Use `--limit` to change that count. Interactive terminals highlight keywords and use three high-contrast styles; redirected output and `NO_COLOR` remain plain text.

`update` fetches and fast-forwards the configured registry, then reconciles user links and every tracked project. If the registry retired a selected skill, agentlink removes its managed links and manifest entry, warns with the affected project path, and asks you to review and commit the manifest change. A project with no remaining selected skills is untracked. Unmanaged local skills are left alone.

The installed session-start integrations call `agentlink update-if-due`; it returns silently unless no check has run in the last 15 minutes. Successful reconciliation warnings are forwarded to the session, while update failures warn without blocking startup. Run `init` only for first-time setup.

## Configuration

`~/.config/agentlink/config.toml` contains only machine-specific state:

```toml
registry = "ssh://git@gitlab.example/apps/agent-config.git"
agents = ["claude", "codex", "pi"]
skills = ["rust"]
projects = ["/home/you/projects/my-app"]
```

Agentlink relies on the installed `git` command and your existing Git credentials. It has no daemon, Git library, database, memory layer, prompt translator, or provider-specific skill copies.

Generated hook assets live under `~/.local/share/agentlink/integrations`. Run `agentlink install-hooks` after changing the enabled agents or upgrading agentlink's bundled integration; it installs enabled hooks and removes disabled hooks. Run `agentlink uninstall-hooks` to remove every agentlink session hook and its generated assets.

## Develop and test

Run from the repository root with Rust, rustfmt, Clippy and Git installed:

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
sh tests/install.sh
```

The installation test builds a release binary into a temporary prefix, initializes a local registry, verifies links and updates, then verifies offline operation. It uses an isolated home and Git identity. Default Cargo tests require no third-party agent CLIs; provider CLI tests are ignored until those tools are installed.

To run the same checks without a host Rust toolchain, use Docker:

```sh
docker run --rm -v "$PWD:/source:ro" rust:1.99-bookworm sh -ec 'cp -a /source /tmp/agentlink; cd /tmp/agentlink; rustup component add rustfmt clippy; cargo fmt --check; cargo clippy --locked --all-targets -- -D warnings; cargo test --locked; sh tests/install.sh'
```

For the complete provider suite, build the test image with Docker Buildx:

```sh
docker buildx bake --load
docker run --rm agentlink-integration:local
```

The entrypoint installs the latest Claude, Codex, Gemini and Pi CLIs from npm, logs their versions, and runs all integration tests sequentially. It needs internet access, but no host home directory, credentials or live model account. Upstream CLI changes can affect results between runs. To run only the core suite in the same image, skip the CLI-installing entrypoint:

```sh
docker run --rm --entrypoint /usr/local/libexec/agentlink-integration-tests agentlink-integration:local --test-threads=1
```

`IMAGE_REPOSITORY` and `IMAGE_TAG` configure the Bake tag (`CI_COMMIT_SHA` overrides the latter for CI). Publishing is explicit with `--push`; the default build does not require a registry or private infrastructure. `tests/integration/run` is an optional Kubernetes launcher: set `IMAGE_REPOSITORY` to a registry reachable by your builder and cluster, select a builder with `BUILDX_BUILDER`, and optionally set `BAKE_DEFAULTS` and `AGENTLINK_TEST_NAMESPACE`. It requires Docker Buildx, kubectl, jq, registry access and permission to create the test namespace and Jobs.

## License

[CC BY-NC 4.0](LICENSE) — free for non-commercial use with attribution. Commercial use requires a separate paid license from the copyright holder. Third-party dependencies retain their own licenses.
