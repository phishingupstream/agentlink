use anstyle::{AnsiColor, Effects, Style};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::ErrorKind;
use std::io::Write as IoWrite;
use std::io::{self, IsTerminal};
use std::num::NonZeroUsize;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, ExitCode, Output};
use std::time::Duration;

const PROJECT_CONFIG: &str = ".agentlink.toml";
const INSTRUCTIONS: &str = "global/AGENTS.md";
const SKILLS_DIR: &str = "skills";
const UPDATE_CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
const UPDATE_CHECK_FILE: &str = "last-update-check";
const UPDATE_LOCK_FILE: &str = "update-check.lock";
const INTEGRATIONS_DIR: &str = "integrations";
const INTEGRATION_FILES: [(&str, &str); 7] = [
    (
        ".agents/plugins/marketplace.json",
        include_str!("../integrations/.agents/plugins/marketplace.json"),
    ),
    (
        ".claude-plugin/marketplace.json",
        include_str!("../integrations/.claude-plugin/marketplace.json"),
    ),
    (
        "plugins/agentlink/.claude-plugin/plugin.json",
        include_str!("../integrations/plugins/agentlink/.claude-plugin/plugin.json"),
    ),
    (
        "plugins/agentlink/.codex-plugin/plugin.json",
        include_str!("../integrations/plugins/agentlink/.codex-plugin/plugin.json"),
    ),
    (
        "plugins/agentlink/hooks/hooks.json",
        include_str!("../integrations/plugins/agentlink/hooks/hooks.json"),
    ),
    (
        "plugins/agentlink/adapters/pi.ts",
        include_str!("../integrations/plugins/agentlink/adapters/pi.ts"),
    ),
    (
        "plugins/agentlink/gemini-extension.json",
        include_str!("../integrations/plugins/agentlink/gemini-extension.json"),
    ),
];

#[derive(Parser)]
#[command(
    version,
    about = "Keep shared agent instructions and selected Agent Skills available offline"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Show project skill status.
    Status,
    /// Clone a remote registry and create the user configuration.
    Init {
        /// Git URL for the authoritative registry.
        registry: String,
        /// Enabled coding agents.
        #[arg(long, value_delimiter = ',', default_value = "claude,codex,pi")]
        agents: Vec<Agent>,
    },
    /// Fetch and fast-forward the registry, then reconcile all links.
    Update,
    /// Update the registry and links if no check ran in the last 15 minutes.
    UpdateIfDue,
    /// Install session-start update hooks for the enabled coding agents.
    InstallHooks,
    /// Remove all agentlink session-start update hooks.
    UninstallHooks,
    /// Reconcile user and tracked-project links without network access.
    Sync,
    /// Search cached skill contents and list matching skills.
    Search {
        query: String,
        /// Maximum number of skills to show.
        #[arg(short = 'n', long, default_value = "10")]
        limit: NonZeroUsize,
    },
    /// Find unmanaged project-local skills in an app directory.
    Discover {
        /// Project directory, or a directory containing projects.
        #[arg(default_value = ".")]
        root: PathBuf,
    },
    /// Move a project-local skill into the shared registry and manage it.
    Migrate {
        skill: String,
        #[arg(long, default_value = ".")]
        project: PathBuf,
        /// Select the source when the skill exists in more than one local path.
        #[arg(long)]
        source: Option<PathBuf>,
    },
    /// Select skills for a project or the user, then sync them.
    Use {
        #[arg(required = true)]
        skills: Vec<String>,
        #[arg(long, conflicts_with = "project")]
        user: bool,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Remove project or user skill selections, then sync them.
    Unuse {
        #[arg(required = true)]
        skills: Vec<String>,
        #[arg(long, conflicts_with = "project")]
        user: bool,
        #[arg(long)]
        project: Option<PathBuf>,
    },
    /// Stop tracking a project and remove its agentlink-owned skill links.
    Untrack {
        #[arg(long, default_value = ".")]
        project: PathBuf,
    },
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
enum Agent {
    Claude,
    Codex,
    Pi,
    Gemini,
}

const ALL_AGENTS: [Agent; 4] = [Agent::Claude, Agent::Codex, Agent::Pi, Agent::Gemini];

const SKILL_DESTINATIONS: [(&str, &[Agent]); 2] = [
    (".claude/skills", &[Agent::Claude]),
    (".agents/skills", &[Agent::Codex, Agent::Pi, Agent::Gemini]),
];
const RETIRED_SKILL_DESTINATIONS: [(&str, Option<Agent>); 3] = [
    (".pi/skills", Some(Agent::Pi)),
    (".opencode/skills", None),
    (".gemini/skills", Some(Agent::Gemini)),
];
const LOCAL_SKILL_DESTINATIONS: [&str; 5] = [
    ".claude/skills",
    ".agents/skills",
    ".pi/skills",
    ".opencode/skills",
    ".gemini/skills",
];

impl Agent {
    fn instructions_path(self) -> &'static str {
        match self {
            Self::Claude => ".claude/CLAUDE.md",
            Self::Codex => ".codex/AGENTS.md",
            Self::Pi => ".pi/agent/AGENTS.md",
            Self::Gemini => ".gemini/GEMINI.md",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct UserConfig {
    registry: String,
    #[serde(deserialize_with = "deserialize_agents")]
    agents: Vec<Agent>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    projects: Vec<PathBuf>,
}

// Read existing configurations without re-enabling the retired agent.
fn deserialize_agents<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<Agent>, D::Error> {
    Vec::<String>::deserialize(deserializer)?
        .into_iter()
        .filter(|name| name != "opencode")
        .map(|name| Agent::deserialize(serde::de::value::StringDeserializer::<D::Error>::new(name)))
        .collect()
}

#[derive(Debug, Default, Deserialize, Serialize)]
struct ProjectConfig {
    #[serde(default)]
    skills: Vec<String>,
}

#[derive(Default)]
struct SkillMatches {
    excerpts: Vec<SearchExcerpt>,
    total: usize,
}

struct SearchExcerpt {
    location: String,
    content: String,
}

fn main() -> ExitCode {
    if let Err(error) = run(Cli::parse()) {
        eprintln!("agentlink: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn run(cli: Cli) -> Result<(), String> {
    match cli.command.unwrap_or(Command::Status) {
        Command::Status => status(),
        Command::Init { registry, agents } => init(registry, agents),
        Command::Update => {
            let mut config = read_user_config()?;
            let registry = registry_dir()?;
            update_registry(&config, &registry)?;
            reconcile_retired_skills(&mut config, &registry)?;
            sync_all(&config, &registry, &home_dir()?)?;
            Ok(())
        }
        Command::UpdateIfDue => {
            if let Err(error) = update_if_due() {
                eprintln!("agentlink: warning: {error}");
            }
            Ok(())
        }
        Command::InstallHooks => install_hooks(),
        Command::UninstallHooks => uninstall_hooks(),
        Command::Sync => {
            let config = read_user_config()?;
            let registry = checked_registry()?;
            sync_all(&config, &registry, &home_dir()?)?;
            Ok(())
        }
        Command::Search { query, limit } => {
            let registry = checked_registry()?;
            let matches = rank_skills(search_skills(&registry, &query)?, &query, limit.get());
            let color = io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none();
            print!(
                "{}",
                format_search_results(&registry, &matches, &query, color)
            );
            Ok(())
        }
        Command::Discover { root } => discover(root),
        Command::Migrate {
            skill,
            project,
            source,
        } => migrate_skill(skill, project, source),
        Command::Use {
            skills,
            user,
            project,
        } => update_skills(skills, user, project, false),
        Command::Unuse {
            skills,
            user,
            project,
        } => update_skills(skills, user, project, true),
        Command::Untrack { project } => untrack_project(project),
    }
}

fn update_if_due() -> Result<(), String> {
    let config = user_config_path()?;
    if !config.is_file() {
        return Ok(());
    }
    let marker = registry_dir()?.with_file_name(UPDATE_CHECK_FILE);
    if !update_is_due(&marker) {
        return Ok(());
    }
    let lock_path = marker.with_file_name(UPDATE_LOCK_FILE);
    let Some(_lock) = try_update_lock(&lock_path)? else {
        return Ok(());
    };
    if !update_is_due(&marker) {
        return Ok(());
    }
    fs::write(&marker, "").map_err(|error| {
        format!(
            "cannot record update check in {}: {error}",
            marker.display()
        )
    })?;
    let executable = env::current_exe()
        .map_err(|error| format!("cannot resolve agentlink executable: {error}"))?;
    let output = ProcessCommand::new(executable)
        .arg("update")
        .output()
        .map_err(|error| format!("cannot run agentlink update: {error}"))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "update check failed: {}",
            detail.trim().trim_start_matches("agentlink: ")
        ));
    }
    io::stderr()
        .write_all(&output.stderr)
        .map_err(|error| format!("cannot forward update warnings: {error}"))?;
    Ok(())
}

fn try_update_lock(path: &Path) -> Result<Option<fs::File>, String> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map_err(|error| format!("cannot open {}: {error}", path.display()))?;
    match lock.try_lock() {
        Ok(()) => Ok(Some(lock)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(error)) => {
            Err(format!("cannot lock {}: {error}", path.display()))
        }
    }
}

fn update_is_due(marker: &Path) -> bool {
    marker
        .metadata()
        .and_then(|metadata| metadata.modified())
        .map_or(true, |checked_at| {
            checked_at
                .elapsed()
                .map_or(true, |age| age >= UPDATE_CHECK_INTERVAL)
        })
}

fn install_hooks() -> Result<(), String> {
    let config = read_user_config()?;
    let home = home_dir()?;
    let integrations = integration_dir(&home);
    materialize_integrations(&integrations)?;
    let plugin = integrations.join("plugins/agentlink");
    remove_retired_hook(&home)?;

    for agent in ALL_AGENTS {
        if config.agents.contains(&agent) {
            install_agent_hook(agent, &home, &integrations, &plugin)?;
        } else {
            remove_agent_hook(agent, &home, &plugin)?;
        }
    }
    println!("session hooks are installed");
    Ok(())
}

fn uninstall_hooks() -> Result<(), String> {
    let home = home_dir()?;
    let integrations = integration_dir(&home);
    let plugin = integrations.join("plugins/agentlink");
    remove_retired_hook(&home)?;
    for agent in ALL_AGENTS {
        remove_agent_hook(agent, &home, &plugin)?;
    }
    remove_if_exists(&integrations)?;
    println!("session hooks are removed");
    Ok(())
}

fn install_agent_hook(
    agent: Agent,
    home: &Path,
    integrations: &Path,
    plugin: &Path,
) -> Result<(), String> {
    match agent {
        Agent::Claude => {
            add_marketplace("claude", integrations)?;
            run_agent_command("claude", &["plugin", "marketplace", "update", "agentlink"])?;
            run_agent_command(
                "claude",
                &[
                    "plugin",
                    "install",
                    "agentlink@agentlink",
                    "--scope",
                    "user",
                ],
            )?;
            run_agent_command("claude", &["plugin", "update", "agentlink@agentlink"])
        }
        Agent::Codex => {
            add_marketplace("codex", integrations)?;
            run_agent_command("codex", &["plugin", "add", "agentlink@agentlink"])
        }
        Agent::Pi => ensure_link(
            &plugin.join("adapters/pi.ts"),
            &home.join(".pi/agent/extensions/agentlink.ts"),
        ),
        Agent::Gemini => ensure_link(plugin, &home.join(".gemini/extensions/agentlink")),
    }
}

fn remove_agent_hook(agent: Agent, home: &Path, plugin: &Path) -> Result<(), String> {
    match agent {
        Agent::Claude => {
            remove_agent_command(
                "claude",
                &[
                    "plugin",
                    "uninstall",
                    "agentlink@agentlink",
                    "--scope",
                    "user",
                ],
            )?;
            remove_agent_command("claude", &["plugin", "marketplace", "remove", "agentlink"])
        }
        Agent::Codex => {
            remove_agent_command("codex", &["plugin", "remove", "agentlink@agentlink"])?;
            remove_agent_command("codex", &["plugin", "marketplace", "remove", "agentlink"])
        }
        Agent::Pi => remove_link_to(
            &home.join(".pi/agent/extensions/agentlink.ts"),
            &plugin.join("adapters/pi.ts"),
        ),
        Agent::Gemini => remove_link_to(&home.join(".gemini/extensions/agentlink"), plugin),
    }
}

fn remove_retired_hook(home: &Path) -> Result<(), String> {
    let adapter = integration_dir(home).join("plugins/agentlink/adapters/opencode.ts");
    remove_link_to(
        &home.join(".config/opencode/plugins/agentlink.ts"),
        &adapter,
    )?;
    remove_if_exists(&adapter)
}

fn materialize_integrations(destination: &Path) -> Result<(), String> {
    for (relative, content) in INTEGRATION_FILES {
        let path = destination.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        }
        fs::write(&path, content)
            .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn add_marketplace(agent: &str, integrations: &Path) -> Result<(), String> {
    let source = integrations
        .to_str()
        .ok_or_else(|| format!("{} is not valid UTF-8", integrations.display()))?;
    let command = if agent == "claude" {
        vec!["plugin", "marketplace", "add", source, "--scope", "user"]
    } else {
        vec!["plugin", "marketplace", "add", source]
    };
    match run_agent_command(agent, &command) {
        Ok(()) => Ok(()),
        Err(error) if error.to_ascii_lowercase().contains("already") => Ok(()),
        Err(error) => Err(error),
    }
}

fn run_agent_command(program: &str, args: &[&str]) -> Result<(), String> {
    let output = ProcessCommand::new(program)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run {program}: {error}"))?;
    let context = format!("{program} {}", args.join(" "));
    command_result(output, &context).map(|_| ())
}

fn remove_agent_command(program: &str, args: &[&str]) -> Result<(), String> {
    let output = match ProcessCommand::new(program).args(args).output() {
        Ok(output) => output,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(format!("cannot run {program}: {error}")),
    };
    let context = format!("{program} {}", args.join(" "));
    match command_result(output, &context) {
        Ok(_) => Ok(()),
        Err(error)
            if [
                "not installed",
                "not found",
                "does not exist",
                "unknown marketplace",
                "not configured or installed",
            ]
            .iter()
            .any(|missing| error.to_ascii_lowercase().contains(missing)) =>
        {
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn status() -> Result<(), String> {
    let color = io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none();
    let heading = bold_colored(color, AnsiColor::BrightCyan);
    let warning = colored(color, AnsiColor::BrightYellow);
    let config = read_user_config()?;
    let registry = checked_registry()?;
    let user_skills: BTreeSet<_> = config.skills.iter().cloned().collect();
    let (_, user_links_current) = inspect_skills(&config, &user_skills, &registry, &home_dir()?)?;
    let user_current =
        user_links_current && validate_skills(&registry.join(SKILLS_DIR), &config.skills).is_ok();
    println!(
        "{heading}user skills:{heading:#} {}",
        display_skills(&user_skills)
    );
    let user_state = if user_current {
        "current"
    } else {
        "out of date"
    };
    let user_style = if user_current {
        colored(color, AnsiColor::BrightGreen)
    } else {
        warning
    };
    println!("user status: {user_style}{user_state}{user_style:#}");
    if !user_current {
        println!("next: agentlink sync");
    }
    println!();

    let project = canonical_dir(Path::new("."), "project")?;
    let manifest_path = project.join(PROJECT_CONFIG);
    if !manifest_path.exists() {
        println!("{heading}agentlink skills:{heading:#} none");
        println!("status: {warning}not enabled{warning:#}");
        println!("next: agentlink use <skill>");
        return Ok(());
    }

    let manifest = read_project_config(&manifest_path)?;
    let selected: BTreeSet<_> = manifest.skills.iter().cloned().collect();
    let (other, links_current) = inspect_skills(&config, &selected, &registry, &project)?;
    let tracked = config.projects.iter().any(|tracked| tracked == &project);
    let current = links_current
        && tracked
        && validate_skills(&registry.join(SKILLS_DIR), &manifest.skills).is_ok();
    println!(
        "{heading}agentlink skills:{heading:#} {}",
        display_skills(&selected)
    );
    let state = if current { "current" } else { "out of date" };
    let state_style = if current {
        colored(color, AnsiColor::BrightGreen)
    } else {
        warning
    };
    println!("status: {state_style}{state}{state_style:#}");
    if !current {
        if tracked {
            println!("next: agentlink sync");
        } else {
            println!(
                "next: agentlink use {}",
                selected.first().map_or("<skill>", String::as_str)
            );
        }
    }
    let other = other
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if !other.is_empty() {
        let other_style = styled(color, Style::new().effects(Effects::DIMMED));
        println!();
        println!("{other_style}other skills: {other}{other_style:#}");
    }
    Ok(())
}

fn display_skills(skills: &BTreeSet<String>) -> String {
    let skills = skills
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if skills.is_empty() {
        "none".to_owned()
    } else {
        skills
    }
}

fn discover(root: PathBuf) -> Result<(), String> {
    let root = canonical_dir(&root, "discovery root")?;
    let registry = checked_registry()?;
    let skills_root = registry.join(SKILLS_DIR);
    let mut projects = vec![root.clone()];
    for entry in
        fs::read_dir(&root).map_err(|error| format!("cannot read {}: {error}", root.display()))?
    {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            projects.push(path);
        }
    }
    projects.sort();

    let mut found = 0;
    for project in projects {
        let skills = match local_skill_sources(&skills_root, &project) {
            Ok(skills) => skills,
            Err(error) if project != root => {
                eprintln!(
                    "agentlink: warning: cannot inspect {}: {error}",
                    project.display()
                );
                continue;
            }
            Err(error) => return Err(error),
        };
        if skills.is_empty() {
            continue;
        }
        println!("{}", project.display());
        for (name, sources) in skills {
            found += 1;
            let sources = sources
                .iter()
                .map(|source| {
                    source
                        .strip_prefix(&project)
                        .unwrap_or(source)
                        .display()
                        .to_string()
                })
                .collect::<Vec<_>>()
                .join(", ");
            println!("  {name}: {sources}");
        }
    }
    if found == 0 {
        println!("no unmanaged project-local skills found");
    } else {
        println!("\nmigrate with: agentlink migrate <skill> --project <project>");
    }
    Ok(())
}

fn migrate_skill(skill: String, project: PathBuf, source: Option<PathBuf>) -> Result<(), String> {
    validate_skill_name(&skill)?;
    let project = canonical_dir(&project, "project")?;
    let mut config = read_user_config()?;
    let registry = checked_registry()?;
    ensure_git_clean(&registry)?;
    update_registry(&config, &registry)?;

    let skills_root = registry.join(SKILLS_DIR);
    let sources = local_skill_sources(&skills_root, &project)?
        .remove(&skill)
        .ok_or_else(|| {
            format!(
                "no unmanaged local skill `{skill}` found in {}",
                project.display()
            )
        })?;
    let source = match source {
        Some(source) => {
            let source = if source.is_absolute() {
                source
            } else {
                project.join(source)
            };
            if !sources.contains(&source) {
                return Err(format!(
                    "{} is not a discovered source for `{skill}`",
                    source.display()
                ));
            }
            source
        }
        None if sources.len() == 1 => sources[0].clone(),
        None => {
            let choices = sources
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            return Err(format!(
                "multiple local copies of `{skill}` found: {choices}; select one with --source"
            ));
        }
    };
    let destination = skills_root.join(&skill);
    if destination.exists() {
        return Err(format!(
            "registry skill `{skill}` already exists; use `agentlink use {skill} --project {}` instead",
            project.display()
        ));
    }

    if let Err(error) = copy_tree(&source, &destination) {
        remove_if_exists(&destination)?;
        return Err(error);
    }
    let relative = format!("skills/{skill}");
    let commit_message = format!("Migrate {skill}\n\nSource: {}", source_repository(&project));
    if let Err(error) = git_checked_args(&registry, &["add", "--", &relative]).and_then(|_| {
        git_checked_args(
            &registry,
            &[
                "commit",
                "--quiet",
                "--message",
                &commit_message,
                "--",
                &relative,
            ],
        )
    }) {
        remove_if_exists(&destination)?;
        let _ = git_checked_args(&registry, &["add", "-A", "--", &relative]);
        return Err(error);
    }
    git_checked_args(&registry, &["push", "--quiet", "origin", "HEAD"]).map_err(|error| {
        format!(
            "{error}; the registry commit is preserved locally and the project skill was not removed"
        )
    })?;

    let manifest_path = project.join(PROJECT_CONFIG);
    let mut manifest = read_project_config(&manifest_path)?;
    manifest.skills.push(skill.clone());
    manifest.skills.sort_unstable();
    manifest.skills.dedup();
    write_toml(&manifest_path, &manifest)?;
    track_project(&mut config, &project)?;
    for local in sources {
        remove_if_exists(&local)?;
    }
    sync_project(&config, &manifest, &registry, &project)?;
    println!(
        "migrated `{skill}` from {} to the shared registry",
        source.display()
    );
    println!("review and commit {}", manifest_path.display());
    Ok(())
}

fn source_repository(project: &Path) -> String {
    ProcessCommand::new("git")
        .arg("-C")
        .arg(project)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| String::from_utf8(output.stdout).ok())
        .map(|remote| remote.trim().to_owned())
        .filter(|remote| !remote.is_empty())
        .unwrap_or_else(|| project.display().to_string())
}

fn local_skill_sources(
    skills_root: &Path,
    project: &Path,
) -> Result<BTreeMap<String, Vec<PathBuf>>, String> {
    let mut skills = BTreeMap::<String, Vec<PathBuf>>::new();
    for destination in LOCAL_SKILL_DESTINATIONS {
        let destination = project.join(destination);
        let entries = match fs::read_dir(&destination) {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => continue,
            Err(error) => return Err(format!("cannot read {}: {error}", destination.display())),
        };
        for entry in entries {
            let path = entry.map_err(|error| error.to_string())?.path();
            let Some(name) = path.file_name().and_then(OsStr::to_str) else {
                continue;
            };
            let managed = fs::read_link(&path).is_ok_and(|target| target == skills_root.join(name));
            if !managed && path.join("SKILL.md").is_file() {
                skills.entry(name.to_owned()).or_default().push(path);
            }
        }
    }
    Ok(skills)
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir(destination)
        .map_err(|error| format!("cannot create {}: {error}", destination.display()))?;
    for entry in fs::read_dir(source)
        .map_err(|error| format!("cannot read {}: {error}", source.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let source = entry.path();
        let destination = destination.join(entry.file_name());
        let file_type = entry
            .file_type()
            .map_err(|error| format!("cannot inspect {}: {error}", source.display()))?;
        if file_type.is_dir() {
            copy_tree(&source, &destination)?;
        } else if file_type.is_file() {
            fs::copy(&source, &destination).map_err(|error| {
                format!(
                    "cannot copy {} to {}: {error}",
                    source.display(),
                    destination.display()
                )
            })?;
        } else if file_type.is_symlink() {
            let target = fs::read_link(&source)
                .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
            symlink(target, &destination)
                .map_err(|error| format!("cannot link {}: {error}", destination.display()))?;
        } else {
            return Err(format!("unsupported file type in {}", source.display()));
        }
    }
    Ok(())
}

fn update_skills(
    skills: Vec<String>,
    user: bool,
    project: Option<PathBuf>,
    remove: bool,
) -> Result<(), String> {
    if user {
        update_user(skills, remove)
    } else {
        update_project(
            skills,
            project.unwrap_or_else(|| PathBuf::from(".")),
            remove,
        )
    }
}

fn update_user(skills: Vec<String>, remove: bool) -> Result<(), String> {
    let mut config = read_user_config()?;
    let registry = checked_registry()?;
    let changes: BTreeSet<_> = skills.into_iter().collect();
    if remove {
        config.skills.retain(|skill| !changes.contains(skill));
    } else {
        config.skills.extend(changes);
        config.skills.sort_unstable();
        config.skills.dedup();
        validate_skills(&registry.join(SKILLS_DIR), &config.skills)?;
    }
    let path = user_config_path()?;
    write_toml(&path, &config)?;
    println!("updated {}", path.display());
    sync_user(&config, &registry, &home_dir()?)
}

fn update_project(skills: Vec<String>, project: PathBuf, remove: bool) -> Result<(), String> {
    let project = canonical_dir(&project, "project")?;
    let path = project.join(PROJECT_CONFIG);
    let mut manifest = read_project_config(&path)?;
    let registry = checked_registry()?;
    let changes: BTreeSet<_> = skills.into_iter().collect();
    if remove {
        manifest.skills.retain(|skill| !changes.contains(skill));
    } else {
        manifest.skills.extend(changes);
        manifest.skills.sort_unstable();
        manifest.skills.dedup();
        validate_skills(&registry.join(SKILLS_DIR), &manifest.skills)?;
    }
    write_toml(&path, &manifest)?;
    println!("updated {}", path.display());
    let mut config = read_user_config()?;
    if !remove {
        track_project(&mut config, &project)?;
    } else if manifest.skills.is_empty() {
        forget_project(&mut config, &project)?;
    }
    sync_project(&config, &manifest, &registry, &project)
}

fn untrack_project(project: PathBuf) -> Result<(), String> {
    let project = canonical_dir(&project, "project")?;
    let mut config = read_user_config()?;
    forget_project(&mut config, &project)?;
    let registry = checked_registry()?;
    sync_project(&config, &ProjectConfig::default(), &registry, &project)?;
    println!("untracked {}", project.display());
    Ok(())
}

fn init(registry: String, agents: Vec<Agent>) -> Result<(), String> {
    let config_path = user_config_path()?;
    let cache = registry_dir()?;
    if config_path.exists() || cache.exists() {
        return Err("agentlink is already initialized".to_owned());
    }
    if let Some(parent) = cache.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    // Git resolves a local origin relative to its checkout during later fetches.
    let registry = if Path::new(&registry).exists() {
        canonical_dir(Path::new(&registry), "registry")?
            .to_string_lossy()
            .into_owned()
    } else {
        registry
    };
    git_clone(&registry, &cache)?;
    if let Err(error) = validate_registry(&cache) {
        remove(&cache)?;
        return Err(error);
    }
    let config = UserConfig {
        registry,
        agents: agents
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
        skills: Vec::new(),
        projects: Vec::new(),
    };
    write_toml(&config_path, &config)?;
    println!("wrote {}", config_path.display());
    sync_user(&config, &cache, &home_dir()?)?;
    Ok(())
}

fn update_registry(config: &UserConfig, registry: &Path) -> Result<(), String> {
    if !registry.join(".git").is_dir() {
        return Err(format!("{} is not a Git checkout", registry.display()));
    }
    git_checked_args(
        registry,
        &["remote", "set-url", "origin", config.registry.as_str()],
    )?;
    git_checked_args(registry, &["fetch", "--quiet", "origin"])?;
    git_checked_args(registry, &["merge", "--ff-only", "--quiet", "@{upstream}"])?;
    validate_registry(registry)?;
    println!("updated {}", registry.display());
    Ok(())
}

fn reconcile_retired_skills(config: &mut UserConfig, registry: &Path) -> Result<(), String> {
    let skills_root = registry.join(SKILLS_DIR);
    let retired = retired_skills(&skills_root, &config.skills);
    if !retired.is_empty() {
        config.skills.retain(|skill| !retired.contains(skill));
        let config_path = user_config_path()?;
        write_toml(&config_path, config)?;
        sync_user(config, registry, &home_dir()?)?;
        eprintln!(
            "agentlink: warning: removed retired user skill(s) {} from {}",
            retired
                .iter()
                .map(|skill| format!("`{skill}`"))
                .collect::<Vec<_>>()
                .join(", "),
            config_path.display()
        );
    }

    let mut untrack = Vec::new();
    for configured_project in config.projects.clone() {
        let project = match canonical_dir(&configured_project, "tracked project") {
            Ok(project) => project,
            Err(_) => continue,
        };
        let manifest_path = project.join(PROJECT_CONFIG);
        if !manifest_path.is_file() {
            continue;
        }
        let mut manifest = match read_project_config(&manifest_path) {
            Ok(manifest) => manifest,
            Err(_) => continue,
        };
        let retired = retired_skills(&skills_root, &manifest.skills);
        if retired.is_empty() {
            continue;
        }

        manifest.skills.retain(|skill| !retired.contains(skill));
        write_toml(&manifest_path, &manifest)?;
        sync_project(config, &manifest, registry, &project)?;
        let retired = retired
            .iter()
            .map(|skill| format!("`{skill}`"))
            .collect::<Vec<_>>()
            .join(", ");
        eprintln!(
            "agentlink: warning: removed retired skill(s) {retired} from {}; review and commit the manifest change",
            manifest_path.display()
        );
        if manifest.skills.is_empty() {
            untrack.push(project);
        }
    }
    for project in untrack {
        forget_project(config, &project)?;
    }
    Ok(())
}

fn retired_skills(skills_root: &Path, skills: &[String]) -> BTreeSet<String> {
    skills
        .iter()
        .filter(|skill| {
            validate_skill_name(skill).is_ok()
                && !skills_root.join(skill).join("SKILL.md").is_file()
        })
        .cloned()
        .collect()
}

fn git_clone(remote: &str, destination: &Path) -> Result<(), String> {
    let output = ProcessCommand::new("git")
        .args(["clone", "--quiet", "--", remote])
        .arg(destination)
        .output()
        .map_err(|error| format!("cannot run git clone: {error}"))?;
    command_result(output, "git clone")?;
    println!("cloned {remote} -> {}", destination.display());
    Ok(())
}

fn git_checked_args(repository: &Path, args: &[&str]) -> Result<Output, String> {
    let context = format!("git {}", args.join(" "));
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(repository)
        .args(args)
        .output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    command_result(output, &context)
}

fn ensure_git_clean(repository: &Path) -> Result<(), String> {
    let output = git_checked_args(repository, &["status", "--porcelain"])?;
    if output.stdout.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} has uncommitted changes; resolve them before migrating a skill",
            repository.display()
        ))
    }
}

fn command_result(output: Output, context: &str) -> Result<Output, String> {
    if output.status.success() {
        Ok(output)
    } else {
        let detail = String::from_utf8_lossy(&output.stderr);
        Err(format!("{context} failed: {}", detail.trim()))
    }
}

fn search_skills(registry: &Path, query: &str) -> Result<BTreeMap<String, SkillMatches>, String> {
    if query.is_empty() {
        return Err("search query cannot be empty".to_owned());
    }
    let output = ProcessCommand::new("git")
        .arg("-C")
        .arg(registry)
        .args(["grep", "-FinIw", "-e"])
        .arg(query)
        .args(["--", SKILLS_DIR])
        .output()
        .map_err(|error| format!("cannot run git grep: {error}"))?;
    if !matches!(output.status.code(), Some(0 | 1)) {
        return command_result(output, "git grep").map(|_| BTreeMap::new());
    }
    let mut skills = BTreeMap::<String, SkillMatches>::new();
    for matched in String::from_utf8_lossy(&output.stdout).lines() {
        let Some((path, matched)) = matched.split_once(':') else {
            continue;
        };
        let Some((skill, path)) = path
            .strip_prefix("skills/")
            .and_then(|path| path.split_once('/'))
        else {
            continue;
        };
        let matches = skills.entry(skill.to_owned()).or_default();
        let Some((line, content)) = matched.split_once(':') else {
            continue;
        };
        if path == "SKILL.md" && content.starts_with("description: ") {
            continue;
        }
        matches.total += 1;
        if matches.excerpts.len() < 3 {
            matches.excerpts.push(SearchExcerpt {
                location: format!("{path}:{line}"),
                content: content.trim().to_owned(),
            });
        }
    }
    let lowercase_query = query.to_ascii_lowercase();
    let skill_entries = fs::read_dir(registry.join(SKILLS_DIR))
        .map_err(|error| format!("cannot list registry skills: {error}"))?;
    for entry in skill_entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        if path.join("SKILL.md").is_file() && name.to_ascii_lowercase().contains(&lowercase_query) {
            skills.entry(name.to_owned()).or_default();
        }
    }
    Ok(skills)
}

fn rank_skills(
    skills: BTreeMap<String, SkillMatches>,
    query: &str,
    limit: usize,
) -> Vec<(String, SkillMatches)> {
    let query = query.to_ascii_lowercase();
    let mut skills = skills
        .into_iter()
        .map(|(name, matches)| {
            let lowercase = name.to_ascii_lowercase();
            let rank = if lowercase == query {
                0
            } else if lowercase.contains(&query) {
                1
            } else {
                2
            };
            (rank, name, matches)
        })
        .collect::<Vec<_>>();
    skills.sort_unstable_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.2.total.cmp(&a.2.total))
            .then_with(|| a.1.cmp(&b.1))
    });
    skills.truncate(limit);
    skills
        .into_iter()
        .map(|(_, name, matches)| (name, matches))
        .collect()
}

fn format_search_results(
    registry: &Path,
    skills: &[(String, SkillMatches)],
    query: &str,
    color: bool,
) -> String {
    let skill_style = bold_colored(color, AnsiColor::BrightCyan);
    let location_style = colored(color, AnsiColor::BrightBlue);
    let more_style = colored(color, AnsiColor::BrightYellow);
    let mut output = String::new();
    for (index, (skill, matches)) in skills.iter().enumerate() {
        if index > 0 {
            output.push('\n');
        }
        writeln!(output, "{skill_style}{skill}{skill_style:#}").unwrap();
        if let Some(description) = skill_description(registry, skill) {
            output.push_str("  ");
            write_highlighted(&mut output, &description, query, more_style);
            output.push('\n');
        }
        output.push('\n');
        for excerpt in &matches.excerpts {
            writeln!(
                output,
                "  • {location_style}{}{location_style:#}",
                excerpt.location
            )
            .unwrap();
            output.push_str("    ");
            write_highlighted(&mut output, &excerpt.content, query, more_style);
            output.push('\n');
        }
        let remaining = matches.total - matches.excerpts.len();
        if remaining > 0 {
            writeln!(
                output,
                "  {more_style}+{remaining} more matches{more_style:#}"
            )
            .unwrap();
        }
    }
    output
}

fn write_highlighted(output: &mut String, text: &str, query: &str, style: Style) {
    let lowercase = text.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    if query.is_empty() {
        output.push_str(text);
        return;
    }
    let mut end = 0;
    for (start, matched) in lowercase.match_indices(&query) {
        let finish = start + matched.len();
        let boundary = |character: Option<char>| {
            character.is_none_or(|character| !character.is_alphanumeric() && character != '_')
        };
        if !boundary(text[..start].chars().next_back()) || !boundary(text[finish..].chars().next())
        {
            continue;
        }
        output.push_str(&text[end..start]);
        write!(output, "{style}{}{style:#}", &text[start..finish]).unwrap();
        end = finish;
    }
    output.push_str(&text[end..]);
}

fn styled(enabled: bool, style: Style) -> Style {
    if enabled { style } else { Style::new() }
}

fn colored(enabled: bool, color: AnsiColor) -> Style {
    styled(enabled, Style::new().fg_color(Some(color.into())))
}

fn bold_colored(enabled: bool, color: AnsiColor) -> Style {
    styled(
        enabled,
        Style::new()
            .fg_color(Some(color.into()))
            .effects(Effects::BOLD),
    )
}

fn skill_description(registry: &Path, skill: &str) -> Option<String> {
    fs::read_to_string(registry.join(SKILLS_DIR).join(skill).join("SKILL.md"))
        .ok()?
        .lines()
        .skip(1)
        .take_while(|line| *line != "---")
        .find_map(|line| line.strip_prefix("description: ").map(str::to_owned))
}

fn sync_all(config: &UserConfig, registry: &Path, home: &Path) -> Result<(), String> {
    sync_user(config, registry, home)?;
    let mut failed = 0;
    for project in &config.projects {
        let project = match canonical_dir(project, "tracked project") {
            Ok(project) => project,
            Err(error) => {
                eprintln!("agentlink: warning: {error}");
                continue;
            }
        };
        let manifest_path = project.join(PROJECT_CONFIG);
        if !manifest_path.is_file() {
            eprintln!("agentlink: warning: {} is missing", manifest_path.display());
            continue;
        }
        let manifest = match read_project_config(&manifest_path) {
            Ok(manifest) => manifest,
            Err(error) => {
                eprintln!("agentlink: warning: {error}");
                failed += 1;
                continue;
            }
        };
        if let Err(error) = sync_project(config, &manifest, registry, &project) {
            eprintln!(
                "agentlink: warning: cannot sync tracked project {}: {error}",
                project.display()
            );
            failed += 1;
        }
    }
    if failed > 0 {
        return Err(format!("{failed} tracked project(s) could not be synced"));
    }
    println!("links are current");
    Ok(())
}

fn sync_user(config: &UserConfig, registry: &Path, home: &Path) -> Result<(), String> {
    let instructions = fs::canonicalize(registry)
        .map_err(|error| format!("cannot resolve registry directory: {error}"))?
        .join(INSTRUCTIONS);
    for agent in ALL_AGENTS {
        let destination = home.join(agent.instructions_path());
        if config.agents.contains(&agent) && instructions.is_file() {
            ensure_link(&instructions, &destination)?;
        } else {
            remove_link_to(&destination, &instructions)?;
        }
    }
    remove_link_to(&home.join(".config/opencode/AGENTS.md"), &instructions)?;
    remove_retired_hook(home)?;
    sync_skills(config, &config.skills, registry, home)
}

fn sync_project(
    user: &UserConfig,
    project_config: &ProjectConfig,
    registry: &Path,
    project: &Path,
) -> Result<(), String> {
    sync_skills(user, &project_config.skills, registry, project)
}

fn sync_skills(
    user: &UserConfig,
    selected: &[String],
    registry: &Path,
    root: &Path,
) -> Result<(), String> {
    let skills_root = registry.join(SKILLS_DIR);
    let selected = validate_skills(&skills_root, selected)?;
    for (destination, agents) in SKILL_DESTINATIONS {
        let desired = if agents.iter().any(|agent| user.agents.contains(agent)) {
            &selected
        } else {
            &BTreeSet::new()
        };
        reconcile_skills(&skills_root, desired, &root.join(destination))?;
    }
    for (destination, _) in RETIRED_SKILL_DESTINATIONS {
        reconcile_skills(&skills_root, &BTreeSet::new(), &root.join(destination))?;
    }
    Ok(())
}

fn inspect_skills(
    user: &UserConfig,
    selected: &BTreeSet<String>,
    registry: &Path,
    root: &Path,
) -> Result<(BTreeSet<String>, bool), String> {
    let skills_root = registry.join(SKILLS_DIR);
    let mut other = BTreeSet::new();
    let mut current = true;
    for (destination, agents) in SKILL_DESTINATIONS {
        let enabled = agents.iter().any(|agent| user.agents.contains(agent));
        let destination = root.join(destination);
        let (managed, local) = installed_skills(&skills_root, &destination)?;
        if enabled {
            other.extend(local);
        }
        current &= if enabled {
            &managed == selected
        } else {
            managed.is_empty()
        };
    }
    for (destination, agent) in RETIRED_SKILL_DESTINATIONS {
        let (managed, local) = installed_skills(&skills_root, &root.join(destination))?;
        if agent.is_some_and(|agent| user.agents.contains(&agent)) {
            other.extend(local);
        }
        current &= managed.is_empty();
    }
    Ok((other, current))
}

fn reconcile_skills(
    skills_root: &Path,
    selected: &BTreeSet<String>,
    destination: &Path,
) -> Result<(), String> {
    let (installed, _) = installed_skills(skills_root, destination)?;
    for name in installed.difference(selected) {
        let path = destination.join(name);
        fs::remove_file(&path)
            .map_err(|error| format!("cannot remove {}: {error}", path.display()))?;
        println!("removed {}", path.display());
    }
    for name in selected.difference(&installed) {
        ensure_link(&skills_root.join(name), &destination.join(name))?;
    }
    Ok(())
}

fn installed_skills(
    skills_root: &Path,
    destination: &Path,
) -> Result<(BTreeSet<String>, BTreeSet<String>), String> {
    let mut managed = BTreeSet::new();
    let mut local = BTreeSet::new();
    let entries = match fs::read_dir(destination) {
        Ok(entries) => entries,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok((managed, local)),
        Err(error) => return Err(format!("cannot read {}: {error}", destination.display())),
    };
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        let Some(name) = path.file_name().and_then(OsStr::to_str) else {
            continue;
        };
        let owned = match fs::read_link(&path) {
            Ok(target) => target == skills_root.join(name),
            Err(error) if error.kind() == ErrorKind::InvalidInput => false,
            Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
        };
        if owned {
            managed.insert(name.to_owned());
        } else if path.join("SKILL.md").is_file() {
            local.insert(name.to_owned());
        }
    }
    Ok((managed, local))
}

fn validate_skills(skills_root: &Path, skills: &[String]) -> Result<BTreeSet<String>, String> {
    let selected: BTreeSet<_> = skills.iter().cloned().collect();
    for name in &selected {
        validate_skill_name(name)?;
        let skill_file = skills_root.join(name).join("SKILL.md");
        if !skill_file.is_file() {
            return Err(format!("{} does not exist", skill_file.display()));
        }
    }
    Ok(selected)
}

fn validate_skill_name(name: &str) -> Result<(), String> {
    if name.is_empty() || name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(format!("invalid skill name `{name}`"));
    }
    Ok(())
}

fn track_project(config: &mut UserConfig, project: &Path) -> Result<(), String> {
    if config.projects.iter().any(|tracked| tracked == project) {
        return Ok(());
    }
    config.projects.push(project.to_owned());
    config.projects.sort();
    write_toml(&user_config_path()?, config)?;
    println!("tracking {}", project.display());
    Ok(())
}

fn forget_project(config: &mut UserConfig, project: &Path) -> Result<(), String> {
    let before = config.projects.len();
    config.projects.retain(|tracked| tracked != project);
    if config.projects.len() == before {
        return Ok(());
    }
    write_toml(&user_config_path()?, config)?;
    println!("stopped tracking {}", project.display());
    Ok(())
}

fn ensure_link(source: &Path, destination: &Path) -> Result<(), String> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink()
                && fs::read_link(destination).ok().as_deref() == Some(source)
            {
                return Ok(());
            }
            remove(destination)?;
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("cannot inspect {}: {error}", destination.display()));
        }
    }
    println!("link {} -> {}", destination.display(), source.display());
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    symlink(source, destination)
        .map_err(|error| format!("cannot link {}: {error}", destination.display()))?;
    Ok(())
}

fn remove(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {}: {error}", path.display()))?;
    let result = if metadata.is_dir() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| format!("cannot replace {}: {error}", path.display()))
}

fn remove_if_exists(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(_) => remove(path),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect {}: {error}", path.display())),
    }
}

fn remove_link_to(destination: &Path, source: &Path) -> Result<(), String> {
    match fs::symlink_metadata(destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let target = fs::read_link(destination)
                .map_err(|error| format!("cannot read {}: {error}", destination.display()))?;
            if target != source {
                return Ok(());
            }
            fs::remove_file(destination)
                .map_err(|error| format!("cannot remove {}: {error}", destination.display()))?;
            println!("removed {}", destination.display());
            Ok(())
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("cannot inspect {}: {error}", destination.display())),
    }
}

fn validate_registry(registry: &Path) -> Result<(), String> {
    let skills = registry.join(SKILLS_DIR);
    if !skills.is_dir() {
        return Err(format!("{} does not exist", skills.display()));
    }
    Ok(())
}

fn checked_registry() -> Result<PathBuf, String> {
    let registry = registry_dir()?;
    validate_registry(&registry)?;
    Ok(registry)
}

fn read_user_config() -> Result<UserConfig, String> {
    read_toml(&user_config_path()?)
}

fn read_project_config(path: &Path) -> Result<ProjectConfig, String> {
    if !path.exists() {
        return Ok(ProjectConfig::default());
    }
    read_toml(path)
}

fn read_toml<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    toml::from_str(&content).map_err(|error| format!("invalid {}: {error}", path.display()))
}

fn write_toml<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let content = toml::to_string_pretty(value).map_err(|error| error.to_string())?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    fs::write(path, content).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn user_config_path() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".config/agentlink/config.toml"))
}

fn registry_dir() -> Result<PathBuf, String> {
    Ok(home_dir()?.join(".local/share/agentlink/registry"))
}

fn integration_dir(home: &Path) -> PathBuf {
    home.join(".local/share/agentlink").join(INTEGRATIONS_DIR)
}

fn home_dir() -> Result<PathBuf, String> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "HOME is not set".to_owned())
}

fn canonical_dir(path: &Path, label: &str) -> Result<PathBuf, String> {
    let path = fs::canonicalize(path)
        .map_err(|error| format!("cannot resolve {label} {}: {error}", path.display()))?;
    if !path.is_dir() {
        return Err(format!("{label} {} is not a directory", path.display()));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temporary_dir(name: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("agentlink-{name}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn fixture() -> (PathBuf, PathBuf, PathBuf, UserConfig) {
        let root = temporary_dir("test");
        let home = root.join("home");
        let registry = root.join("registry");
        fs::create_dir_all(registry.join("skills/rust")).unwrap();
        fs::create_dir_all(registry.join("global")).unwrap();
        fs::create_dir_all(&home).unwrap();
        fs::write(registry.join(INSTRUCTIONS), "global rules\n").unwrap();
        fs::write(
            registry.join("skills/rust/SKILL.md"),
            "---\nname: rust\ndescription: Rust work\n---\n",
        )
        .unwrap();
        let config = UserConfig {
            registry: "ssh://example/skills.git".to_owned(),
            agents: vec![Agent::Claude, Agent::Codex],
            skills: Vec::new(),
            projects: Vec::new(),
        };
        (root, home, registry, config)
    }

    fn git(repository: &Path, args: &[&str]) -> Output {
        let output = ProcessCommand::new("git")
            .arg("-C")
            .arg(repository)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        output
    }

    #[test]
    fn global_sync_links_only_enabled_agents() {
        let (root, home, registry, config) = fixture();
        let retired_instructions = home.join(".config/opencode/AGENTS.md");
        let retired_hook = home.join(".config/opencode/plugins/agentlink.ts");
        let adapter = integration_dir(&home).join("plugins/agentlink/adapters/opencode.ts");
        ensure_link(&registry.join(INSTRUCTIONS), &retired_instructions).unwrap();
        fs::create_dir_all(adapter.parent().unwrap()).unwrap();
        fs::write(&adapter, "retired").unwrap();
        ensure_link(&adapter, &retired_hook).unwrap();
        sync_user(&config, &registry, &home).unwrap();
        assert!(!retired_instructions.is_symlink());
        assert!(!retired_hook.is_symlink());
        assert!(!adapter.exists());
        fs::write(&retired_instructions, "unmanaged").unwrap();
        fs::write(&retired_hook, "unmanaged").unwrap();
        sync_user(&config, &registry, &home).unwrap();
        assert_eq!(
            fs::read_to_string(&retired_instructions).unwrap(),
            "unmanaged"
        );
        assert_eq!(fs::read_to_string(&retired_hook).unwrap(), "unmanaged");
        assert_eq!(
            fs::canonicalize(home.join(".claude/CLAUDE.md")).unwrap(),
            fs::canonicalize(registry.join(INSTRUCTIONS)).unwrap()
        );
        assert!(!home.join(".pi/agent/AGENTS.md").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn global_sync_removes_owned_links_when_instructions_are_absent() {
        let (root, home, registry, config) = fixture();
        sync_user(&config, &registry, &home).unwrap();
        fs::remove_file(registry.join(INSTRUCTIONS)).unwrap();

        sync_user(&config, &registry, &home).unwrap();

        assert_eq!(
            fs::symlink_metadata(home.join(".claude/CLAUDE.md"))
                .unwrap_err()
                .kind(),
            ErrorKind::NotFound
        );
        assert_eq!(
            fs::symlink_metadata(home.join(".codex/AGENTS.md"))
                .unwrap_err()
                .kind(),
            ErrorKind::NotFound
        );
        validate_registry(&registry).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn user_sync_reconciles_enabled_agents_and_preserves_other_skills() {
        let (root, home, registry, mut config) = fixture();
        let local = home.join(".claude/skills/local");
        let collision = home.join(".claude/skills/rust");
        fs::create_dir_all(&local).unwrap();
        fs::create_dir_all(&collision).unwrap();
        fs::write(local.join("SKILL.md"), "local\n").unwrap();
        fs::write(collision.join("SKILL.md"), "collision\n").unwrap();
        config.skills.push("rust".to_owned());

        sync_user(&config, &registry, &home).unwrap();

        assert_eq!(
            fs::read_link(&collision).unwrap(),
            registry.join("skills/rust")
        );
        assert!(home.join(".agents/skills/rust").is_symlink());
        assert!(local.join("SKILL.md").is_file());
        assert!(!home.join(".pi/skills/rust").exists());

        config.skills.clear();
        sync_user(&config, &registry, &home).unwrap();
        assert!(!collision.exists());
        assert!(!home.join(".agents/skills/rust").exists());
        assert!(local.join("SKILL.md").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn retired_skills_ignores_invalid_names() {
        let (root, _home, registry, _config) = fixture();
        let retired = retired_skills(
            &registry.join(SKILLS_DIR),
            &[
                "rust".to_owned(),
                "missing".to_owned(),
                "../invalid".to_owned(),
            ],
        );
        assert_eq!(retired, BTreeSet::from(["missing".to_owned()]));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn command_defaults_to_status() {
        let cli = Cli::try_parse_from(["agentlink"]).unwrap();
        assert!(cli.command.is_none());
        assert!(matches!(
            Cli::try_parse_from(["agentlink", "status"])
                .unwrap()
                .command,
            Some(Command::Status)
        ));
        assert!(matches!(
            Cli::try_parse_from(["agentlink", "update-if-due"])
                .unwrap()
                .command,
            Some(Command::UpdateIfDue)
        ));
        assert!(matches!(
            Cli::try_parse_from(["agentlink", "install-hooks"])
                .unwrap()
                .command,
            Some(Command::InstallHooks)
        ));
        assert!(matches!(
            Cli::try_parse_from(["agentlink", "discover", "/data/projects"])
                .unwrap()
                .command,
            Some(Command::Discover { root }) if root == Path::new("/data/projects")
        ));
        assert!(matches!(
            Cli::try_parse_from([
                "agentlink",
                "migrate",
                "local",
                "--project",
                "/tmp/app",
                "--source",
                ".agents/skills/local",
            ])
            .unwrap()
            .command,
            Some(Command::Migrate { skill, project, source })
                if skill == "local"
                    && project == Path::new("/tmp/app")
                    && source == Some(PathBuf::from(".agents/skills/local"))
        ));
        assert!(matches!(
            Cli::try_parse_from(["agentlink", "use", "rust", "--user"])
                .unwrap()
                .command,
            Some(Command::Use { skills, user: true, project: None })
                if skills == ["rust"]
        ));
        assert!(
            Cli::try_parse_from([
                "agentlink",
                "unuse",
                "rust",
                "--user",
                "--project",
                "/tmp/app",
            ])
            .is_err()
        );
    }

    #[test]
    fn old_user_config_defaults_to_no_skills() {
        let config: UserConfig = toml::from_str(
            "registry = 'ssh://example/skills.git'\nagents = ['codex', 'opencode']\nprojects = []\n",
        )
        .unwrap();
        assert!(config.skills.is_empty());
        assert_eq!(config.agents, [Agent::Codex]);
        assert!(!toml::to_string(&config).unwrap().contains("opencode"));
        assert!(
            toml::from_str::<UserConfig>("registry = 'example'\nagents = ['unknown']\n").is_err()
        );
        assert!(
            Cli::try_parse_from(["agentlink", "init", "example", "--agents", "opencode"]).is_err()
        );
    }

    #[test]
    fn local_skill_discovery_ignores_managed_links_and_copy_preserves_tree() {
        let (root, _home, registry, _config) = fixture();
        let project = root.join("project");
        let local = project.join(".agents/skills/local");
        fs::create_dir_all(local.join("references")).unwrap();
        fs::write(local.join("SKILL.md"), "local\n").unwrap();
        fs::write(local.join("references/example.md"), "example\n").unwrap();
        ensure_link(
            &registry.join("skills/rust"),
            &project.join(".claude/skills/rust"),
        )
        .unwrap();

        let discovered = local_skill_sources(&registry.join(SKILLS_DIR), &project).unwrap();
        assert_eq!(discovered.keys().collect::<Vec<_>>(), [&"local"]);
        assert_eq!(discovered["local"], vec![local.clone()]);

        let copied = root.join("copied");
        copy_tree(&local, &copied).unwrap();
        assert_eq!(
            fs::read_to_string(copied.join("references/example.md")).unwrap(),
            "example\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn integration_assets_are_materialized_and_local_adapters_are_linked() {
        let root = temporary_dir("integrations");
        let home = root.join("home");
        let integrations = integration_dir(&home);
        materialize_integrations(&integrations).unwrap();
        let plugin = integrations.join("plugins/agentlink");
        ensure_link(
            &plugin.join("adapters/pi.ts"),
            &home.join(".pi/agent/extensions/agentlink.ts"),
        )
        .unwrap();
        ensure_link(&plugin, &home.join(".gemini/extensions/agentlink")).unwrap();

        assert!(plugin.join("hooks/hooks.json").is_file());
        assert!(home.join(".pi/agent/extensions/agentlink.ts").is_symlink());
        assert!(!plugin.join("adapters/opencode.ts").exists());
        assert!(home.join(".gemini/extensions/agentlink").is_symlink());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_check_is_due_only_after_interval() {
        let root = temporary_dir("update-check");
        let marker = root.join(UPDATE_CHECK_FILE);
        assert!(update_is_due(&marker));
        fs::write(&marker, "").unwrap();
        assert!(!update_is_due(&marker));
        fs::File::options()
            .write(true)
            .open(&marker)
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(
                std::time::SystemTime::now() - UPDATE_CHECK_INTERVAL - Duration::from_secs(1),
            ))
            .unwrap();
        assert!(update_is_due(&marker));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_lock_allows_only_one_checker() {
        let root = temporary_dir("update-lock");
        let path = root.join(UPDATE_LOCK_FILE);
        let first = try_update_lock(&path).unwrap().unwrap();
        assert!(try_update_lock(&path).unwrap().is_none());
        first.unlock().unwrap();
        drop(first);
        assert!(try_update_lock(&path).unwrap().is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_sync_links_selected_skills() {
        let (root, _home, registry, config) = fixture();
        let project = root.join("project");
        fs::create_dir(&project).unwrap();
        for (destination, _) in RETIRED_SKILL_DESTINATIONS {
            ensure_link(
                &registry.join("skills/rust"),
                &project.join(destination).join("rust"),
            )
            .unwrap();
        }
        let project_config = ProjectConfig {
            skills: vec!["rust".to_owned()],
        };
        sync_project(&config, &project_config, &registry, &project).unwrap();
        let link = project.join(".claude/skills/rust");
        assert!(link.is_symlink());
        assert!(project.join(".agents/skills/rust").is_symlink());
        for (destination, _) in RETIRED_SKILL_DESTINATIONS {
            assert_eq!(
                fs::symlink_metadata(project.join(destination).join("rust"))
                    .unwrap_err()
                    .kind(),
                ErrorKind::NotFound
            );
        }
        let local = root.join("local-skill");
        fs::create_dir(&local).unwrap();
        fs::write(local.join("SKILL.md"), "local\n").unwrap();
        let local_link = project.join(".claude/skills/local");
        symlink(&local, &local_link).unwrap();
        let selected = BTreeSet::from(["rust".to_owned()]);
        let (other, current) = inspect_skills(&config, &selected, &registry, &project).unwrap();
        assert!(current);
        assert_eq!(other, BTreeSet::from(["local".to_owned()]));
        fs::remove_file(project.join(".agents/skills/rust")).unwrap();
        assert!(
            !inspect_skills(&config, &selected, &registry, &project)
                .unwrap()
                .1
        );
        fs::remove_dir_all(registry.join("skills/rust")).unwrap();
        reconcile_skills(
            &registry.join(SKILLS_DIR),
            &BTreeSet::new(),
            &project.join(".claude/skills"),
        )
        .unwrap();
        assert_eq!(
            fs::symlink_metadata(link).unwrap_err().kind(),
            ErrorKind::NotFound
        );
        assert!(local_link.is_symlink());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn existing_destination_is_replaced() {
        let (root, home, registry, config) = fixture();
        fs::create_dir_all(home.join(".claude")).unwrap();
        fs::write(home.join(".claude/CLAUDE.md"), "mine\n").unwrap();
        sync_user(&config, &registry, &home).unwrap();
        assert_eq!(
            fs::canonicalize(home.join(".claude/CLAUDE.md")).unwrap(),
            fs::canonicalize(registry.join(INSTRUCTIONS)).unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_lists_matching_skills_not_files() {
        let (root, _home, registry, _config) = fixture();
        fs::create_dir_all(registry.join("skills/rust/references")).unwrap();
        fs::write(
            registry.join("skills/rust/references/profiling.md"),
            "profiling workflow\n",
        )
        .unwrap();
        fs::write(
            registry.join("skills/rust/SKILL.md"),
            "---\nname: rust\ndescription: Rust profiling tools\n---\n\nProfiling guide\n",
        )
        .unwrap();
        fs::create_dir_all(registry.join("skills/python")).unwrap();
        fs::write(
            registry.join("skills/python/SKILL.md"),
            "Python packaging\nAlways trust UTF-8\n",
        )
        .unwrap();
        fs::create_dir_all(registry.join("skills/name-only")).unwrap();
        fs::write(
            registry.join("skills/name-only/SKILL.md"),
            "Unrelated contents\n",
        )
        .unwrap();
        git(&registry, &["init"]);
        git(&registry, &["add", "."]);
        let matches = search_skills(&registry, "PROFILING").unwrap();
        assert_eq!(matches.keys().collect::<Vec<_>>(), [&"rust"]);
        assert_eq!(matches["rust"].total, 2);
        assert_eq!(matches["rust"].excerpts.len(), 2);
        assert_eq!(
            skill_description(&registry, "rust").as_deref(),
            Some("Rust profiling tools")
        );
        assert!(search_skills(&registry, "missing").unwrap().is_empty());
        assert_eq!(
            search_skills(&registry, "name-only")
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [&"name-only"]
        );
        assert_eq!(
            search_skills(&registry, "rust")
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [&"rust"]
        );
        assert!(search_skills(&registry, "").is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn search_ranks_names_then_match_counts_and_limits_results() {
        let skills = BTreeMap::from([
            (
                "cli".to_owned(),
                SkillMatches {
                    total: 8,
                    ..Default::default()
                },
            ),
            (
                "rust".to_owned(),
                SkillMatches {
                    total: 1,
                    ..Default::default()
                },
            ),
            (
                "rust-perf".to_owned(),
                SkillMatches {
                    total: 2,
                    ..Default::default()
                },
            ),
        ]);
        let ranked = rank_skills(skills, "rust", 2);
        assert_eq!(
            ranked
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["rust", "rust-perf"]
        );
    }

    #[test]
    fn search_output_uses_readable_cards() {
        let (root, _home, registry, _config) = fixture();
        fs::create_dir_all(registry.join("skills/rust")).unwrap();
        fs::write(
            registry.join("skills/rust/SKILL.md"),
            "---\nname: rust\ndescription: Rust profiling tools\n---\n",
        )
        .unwrap();
        let skills = vec![(
            "rust".to_owned(),
            SkillMatches {
                excerpts: vec![SearchExcerpt {
                    location: "SKILL.md:7".to_owned(),
                    content: "Profiling guide".to_owned(),
                }],
                total: 3,
            },
        )];
        assert_eq!(
            format_search_results(&registry, &skills, "profiling", false),
            "rust\n  Rust profiling tools\n\n  • SKILL.md:7\n    Profiling guide\n  +2 more matches\n"
        );
        let colored = format_search_results(&registry, &skills, "profiling", true);
        assert!(colored.contains("\u{1b}["));
        assert!(!colored.contains("\u{1b}[2m"));
        assert!(colored.contains("\u{1b}[93mprofiling\u{1b}[0m"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn update_uses_configured_registry() {
        let root = temporary_dir("configured-registry");
        let cache = root.join("cache");
        fs::create_dir(&cache).unwrap();
        git(&cache, &["init"]);
        git(&cache, &["remote", "add", "origin", "/old-registry"]);
        let config = UserConfig {
            registry: "/new-registry".to_owned(),
            agents: Vec::new(),
            skills: Vec::new(),
            projects: Vec::new(),
        };
        assert!(update_registry(&config, &cache).is_err());
        let remote = git(&cache, &["remote", "get-url", "origin"]);
        assert_eq!(
            String::from_utf8_lossy(&remote.stdout).trim(),
            "/new-registry"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_tracked_project_does_not_block_later_projects() {
        let (root, home, registry, mut config) = fixture();
        let project = root.join("z-project");
        fs::create_dir(&project).unwrap();
        write_toml(
            &project.join(PROJECT_CONFIG),
            &ProjectConfig {
                skills: vec!["rust".to_owned()],
            },
        )
        .unwrap();
        config.projects = vec![root.join("a-missing"), project.clone()];
        sync_all(&config, &registry, &home).unwrap();
        assert!(project.join(".claude/skills/rust").is_symlink());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn invalid_tracked_project_does_not_block_later_projects() {
        let (root, home, registry, mut config) = fixture();
        let invalid = root.join("a-invalid");
        let valid = root.join("z-valid");
        fs::create_dir(&invalid).unwrap();
        fs::create_dir(&valid).unwrap();
        fs::write(invalid.join(PROJECT_CONFIG), "not valid toml = [").unwrap();
        write_toml(
            &valid.join(PROJECT_CONFIG),
            &ProjectConfig {
                skills: vec!["rust".to_owned()],
            },
        )
        .unwrap();
        config.projects = vec![invalid, valid.clone()];

        assert!(sync_all(&config, &registry, &home).is_err());
        assert!(valid.join(".claude/skills/rust").is_symlink());
        fs::remove_dir_all(root).unwrap();
    }
}
