# AgentLink requirements

Approved product behavior for the public distribution. Requirement numbers match the acceptance tests.

## 1. Keep the authoritative registry available offline

### Requirement
Agentlink must keep a remote Git registry available as a validated local checkout so users can reconcile configured state and inspect skills without network access.

### Why
Coding agents must retain their shared instructions and skills when the remote is unavailable, without introducing a second source of truth.

### Rules
- Use the installed `git` command against the configured remote.
- Accept the fixed registry layout: optional `global/AGENTS.md` and required `skills/<name>/SKILL.md`.
- Fetch remote changes only by fast-forward and refuse divergent local history.
- Preserve skill contents without parsing, translating, or creating provider-specific copies.
- Do not add a daemon, scheduler, database, custom Git implementation, or background watcher.

### Acceptance
- Initialization clones and validates a conforming registry.
- `sync`, `status`, and `search` continue to work when the remote cannot be reached.
- An update advances to a newer remote commit without merging or rewriting local history.
- A malformed registry or divergent checkout fails with an actionable error while retaining existing local data.

### Evidence
Automated unit and integration tests establish offline and update behavior. AI review establishes the architectural boundaries.

## 2. Expose configured content through native agent paths

### Requirement
Agentlink must expose shared instructions and selected skills through the native discovery paths of each configured coding agent.

### Why
Users need one registry to work consistently across Claude, Codex, Gemini, and Pi without maintaining provider-specific copies.

### Rules
- Link optional global instructions only for configured agents and remove agentlink-owned links when instructions disappear or an agent is disabled.
- Link project and user skills into `.claude/skills` for Claude and `.agents/skills` for Codex, Gemini, and Pi.
- Remove agentlink-owned links from retired `.gemini/skills`, `.pi/skills`, and `.opencode/skills` destinations.
- Replace a collision at a selected agentlink-owned destination directly.
- Preserve unrelated unmanaged skills and links.
- Retire OpenCode support: ignore its selection in existing user configurations and remove AgentLink-owned OpenCode instruction and hook links during sync. Preserve unmanaged content.

### Acceptance
- Every configured agent discovers the selected instructions and skills through its native interface.
- Removing global instructions or disabling an agent removes only the matching agentlink-owned links.
- A selected-name collision is replaced by the registry link while unrelated local skills remain unchanged.

### Evidence
Automated unit tests and isolated integration scenarios for all four supported agents determine acceptance.

## 3. Keep user and project skill selection independent

### Requirement
Agentlink must let users select skills independently for their user account and for each project while keeping committed project intent separate from machine-local state.

### Why
A project must declare the skills collaborators need without committing one operator’s registry URL, enabled agents, global selections, or tracked project paths.

### Rules
- Store the registry URL, configured agents, user skills, and tracked project paths in `~/.config/agentlink/config.toml`.
- Store project skill selection in the committed `.agentlink.toml` manifest.
- User and project selections must not add or remove each other.
- Removing the final project skill must stop machine-local tracking.
- `untrack` must remove managed project links without changing the project manifest.

### Acceptance
- The same skill can be selected or removed at either scope without affecting the other.
- A project can be tracked, reconciled, and untracked while its committed manifest remains the source of project intent.
- Repeated selection and removal operations remain idempotent.

### Evidence
Automated unit and integration tests determine acceptance.

## 4. Refresh agent sessions safely

### Requirement
Agentlink must refresh the registry and managed links from enabled agent session-start integrations without delaying or blocking the coding agent when refresh fails.

### Why
Shared configuration should converge automatically while agent startup remains dependable during network, registry, or project failures.

### Rules
- Install integrations only for enabled agents and remove disabled or uninstalled agentlink integrations.
- Share one update check interval of 15 minutes across all agent integrations.
- Allow only one concurrent due check to perform the update.
- Forward successful reconciliation warnings to the session.
- Convert update failures into warnings and return success to the calling agent.

### Acceptance
- Claude, Codex, Gemini, and Pi invoke the bundled integration when enabled.
- Repeated starts inside the interval do not perform another update.
- Concurrent starts do not perform duplicate updates.
- An unavailable remote or invalid tracked project warns without preventing agent startup.

### Evidence
Automated unit tests and isolated integration scenarios determine mechanics. AI review confirms that failure handling remains non-blocking.

## 5. Reconcile retired registry skills without collateral damage

### Requirement
Agentlink must reconcile skills removed from the authoritative registry across user selections and every tracked project without deleting unmanaged content.

### Why
Registry retirement must converge local state instead of permanently blocking updates or silently leaving agents with obsolete skills.

### Rules
- Remove retired skills from user configuration, project manifests, and agentlink-owned links.
- Warn with each affected project and ask the user to review and commit changed manifests.
- Stop tracking a project when no selected skills remain.
- Preserve unrelated unmanaged skills.
- Continue reconciling later tracked projects when an earlier path is missing or invalid; report aggregate failures that prevent reconciliation.

### Acceptance
- Deleting a selected registry skill and running the session update path removes its managed state at both scopes.
- The user receives visible manifest guidance and no empty project remains tracked.
- Unmanaged skills survive retirement.
- One unavailable tracked project does not prevent later valid projects from being reconciled.

### Evidence
Automated unit and integration tests determine acceptance.

## 6. Migrate local skills without losing content or provenance

### Requirement
Agentlink must discover unmanaged project-local skills and migrate a chosen skill into the authoritative registry without losing content, source attribution, or a recoverable local copy before publication succeeds.

### Why
Users need to consolidate existing skills safely instead of manually copying directories and repairing links across coding agents.

### Rules
- Discover the requested root and its immediate child directories across current and retired skill paths.
- Ignore links already managed by the configured registry.
- Require a valid skill name, a clean registry checkout, a fast-forward update, and a unique registry skill name.
- Require explicit source selection when multiple unmanaged copies exist.
- Preserve the complete skill directory, including nested files and symbolic links, without transforming its contents.
- Record the source repository remote, or its local path when no remote exists, in the migration commit.
- Replace local copies with managed links only after the registry push succeeds.

### Acceptance
- Discovery reports each unmanaged skill and its source paths.
- A successful migration publishes an attributed, byte-preserving tree, updates project selection, and installs managed links.
- A validation, commit, or push failure leaves the project-local skill in place and reports recoverable registry state.

### Evidence
Automated integration tests establish the successful lifecycle. Unit tests and AI review cover loss prevention and failure boundaries.

## 7. Inspect cached skills and reconciliation state offline

### Requirement
Agentlink must let users inspect selected skills, reconciliation state, unmanaged skills, and relevant cached skill contents without contacting the remote registry.

### Why
Users need to understand what each agent can discover and how to repair drift even when the registry is unavailable.

### Rules
- Running `agentlink` without a subcommand must show status.
- Status must distinguish user selection, project selection, current or out-of-date state, an unenabled project, and unmanaged local skills.
- Search must perform case-insensitive whole-word matching across cached skill contents, rank matching skill names before content-only matches, honor the result limit, and remain readable with color disabled or redirected output.
- Inspection must not mutate configuration or managed links.

### Acceptance
- Status identifies current state and gives the next reconciliation or selection command when action is required.
- Search returns relevant skill-level results rather than raw matching files and works while the remote is unavailable.
- Redirected output contains no terminal color sequences.

### Evidence
Automated unit and integration tests establish deterministic output and offline behavior. AI review evaluates whether guidance remains actionable.
