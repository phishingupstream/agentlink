#!/bin/sh
# Fresh source install and offline setup, without provider CLIs or credentials.
set -eu
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
cargo install --path . --locked --root "$work/install"
export PATH="$work/install/bin:$PATH"
export HOME="$work/home"
export GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null
mkdir -p "$HOME" "$work/registry/skills/example" "$work/project"
printf '%s\n' '---' 'name: example' 'description: Installation test skill' '---' '# Example' > "$work/registry/skills/example/SKILL.md"
git -C "$work/registry" init -q
git -C "$work/registry" add .
git -C "$work/registry" -c user.name=Test -c user.email=test@example.invalid commit -qm initial
cd "$work"
agentlink --version
agentlink init ./registry --agents codex
cd project
agentlink use example
test -L .agents/skills/example
cmp .agents/skills/example/SKILL.md ../registry/skills/example/SKILL.md
agentlink update
mv ../registry ../offline-registry
agentlink sync
agentlink search example | grep example
agentlink unuse example
test ! -L .agents/skills/example
printf '%s\n' 'Source installation and offline setup passed.'
