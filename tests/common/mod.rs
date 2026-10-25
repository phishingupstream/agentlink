use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use assert_cmd::{Command, assert::Assert, cargo::cargo_bin};
use assert_fs::TempDir;
use serde::{Deserialize, Serialize};

pub const GLOBAL_INSTRUCTIONS: &str = "global/AGENTS.md";
pub const USER_CONFIG: &str = ".config/agentlink/config.toml";
pub const PROJECT_CONFIG: &str = ".agentlink.toml";
pub const CLAUDE_SKILLS: &str = ".claude/skills";
pub const SHARED_SKILLS: &str = ".agents/skills";
pub const SKILL_DIRS: [&str; 2] = [CLAUDE_SKILLS, SHARED_SKILLS];
pub const LEGACY_SKILL_DIRS: [&str; 3] = [".gemini/skills", ".pi/skills", ".opencode/skills"];
pub const INTEGRATIONS_DIR: &str = ".local/share/agentlink/integrations";
pub const PLUGIN_NAME: &str = "agentlink";
pub const PLUGIN_ID: &str = "agentlink@agentlink";
const REGISTRY_DIR: &str = ".local/share/agentlink/registry";
const HOOKS_FILE: &str = "plugins/agentlink/hooks/hooks.json";
const GIT_NAME: &str = "integration";
const GIT_EMAIL: &str = "integration@example.invalid";

// Read the public configuration format rather than importing private implementation types.
#[derive(Debug, Deserialize, Serialize)]
pub struct UserConfig {
    pub registry: String,
    pub agents: Vec<String>,
    pub skills: Vec<String>,
    pub projects: Vec<PathBuf>,
}

#[derive(Debug, Deserialize)]
pub struct ProjectConfig {
    pub skills: Vec<String>,
}

pub struct Fixture {
    _root: TempDir,
    pub home: PathBuf,
    pub source: PathBuf,
    pub remote: PathBuf,
    pub project: PathBuf,
    pub registry: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let root = TempDir::new().unwrap();
        let home = root.path().join("home");
        let source = root.path().join("source");
        let remote = root.path().join("registry.git");
        let project = root.path().join("project");
        let registry = home.join(REGISTRY_DIR);
        let fixture = Self {
            _root: root,
            home,
            source,
            remote,
            project,
            registry,
        };
        for path in [&fixture.home, &fixture.project] {
            std::fs::create_dir_all(path).unwrap();
        }
        write(fixture.source.join(GLOBAL_INSTRUCTIONS), "global-v1\n");
        write_skill(fixture.source.join("skills"), "alpha", "");
        write_skill(fixture.source.join("skills"), "beta", "beta-v1\n");
        fixture.run_git(
            &fixture.source,
            ["init", "--quiet", "--initial-branch=main"],
        );
        fixture.commit("fixture");
        fixture
            .command("git")
            .args(["clone", "--quiet", "--bare"])
            .arg(&fixture.source)
            .arg(&fixture.remote)
            .assert()
            .success();
        fixture
            .git(&fixture.source)
            .args(["remote", "add", "origin"])
            .arg(&fixture.remote)
            .assert()
            .success();
        fixture
    }

    pub fn for_agents(agents: &str) -> Self {
        let fixture = Self::new();
        fixture
            .agentlink()
            .arg("init")
            .arg(&fixture.remote)
            .args(["--agents", agents])
            .assert()
            .success();
        fixture
    }

    pub fn command(&self, executable: impl AsRef<OsStr>) -> Command {
        let mut command = Command::new(executable);
        command
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("NO_COLOR", "1")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_NAME", GIT_NAME)
            .env("GIT_AUTHOR_EMAIL", GIT_EMAIL)
            .env("GIT_COMMITTER_NAME", GIT_NAME)
            .env("GIT_COMMITTER_EMAIL", GIT_EMAIL);
        command
    }

    pub fn agentlink(&self) -> Command {
        self.command(
            std::env::var_os("AGENTLINK_BIN")
                .map(PathBuf::from)
                .unwrap_or_else(|| cargo_bin!("agentlink").into()),
        )
    }

    pub fn run(&self, args: impl IntoIterator<Item = impl AsRef<OsStr>>) -> Assert {
        self.agentlink().args(args).assert().success()
    }

    pub fn git(&self, directory: &Path) -> Command {
        let mut command = self.command("git");
        command.arg("-C").arg(directory);
        command
    }

    pub fn run_git(
        &self,
        directory: &Path,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Assert {
        self.git(directory).args(args).assert().success()
    }

    pub fn commit(&self, message: &str) {
        self.run_git(&self.source, ["add", "."]);
        self.run_git(&self.source, ["commit", "--quiet", "--message", message]);
    }

    pub fn push(&self, message: &str) {
        self.commit(message);
        self.run_git(&self.source, ["push", "--quiet", "origin", "main"]);
    }

    pub fn config(&self) -> UserConfig {
        toml::from_str(&read(self.home.join(USER_CONFIG))).unwrap()
    }

    pub fn manifest(&self) -> ProjectConfig {
        self.manifest_at(&self.project)
    }

    pub fn manifest_at(&self, project: &Path) -> ProjectConfig {
        toml::from_str(&read(project.join(PROJECT_CONFIG))).unwrap()
    }

    pub fn set_agents(&self, agents: &[&str]) {
        let mut config = self.config();
        config.agents = agents.iter().map(|agent| (*agent).into()).collect();
        write(
            self.home.join(USER_CONFIG),
            &toml::to_string(&config).unwrap(),
        );
    }

    pub fn assert_skill_links(&self, base: &Path, directory: &str, skills: &[&str]) {
        for skill in skills {
            assert_link(
                base.join(directory).join(skill),
                self.registry.join("skills").join(skill),
            );
            for other in SKILL_DIRS.into_iter().filter(|other| *other != directory) {
                assert_absent(base.join(other).join(skill));
            }
        }
    }

    pub fn assert_no_skill_links(&self, base: &Path, skills: &[&str]) {
        for directory in SKILL_DIRS {
            for skill in skills {
                assert_absent(base.join(directory).join(skill));
            }
        }
    }
}

// Share the lifecycle; provider modules own the provider-specific assertions.
pub fn check_hooks(agent: &str, installed: fn(&Fixture), removed: fn(&Fixture)) {
    let fixture = Fixture::for_agents(agent);
    fixture.run(["install-hooks"]);
    assert!(
        fixture
            .home
            .join(INTEGRATIONS_DIR)
            .join(HOOKS_FILE)
            .is_file()
    );
    installed(&fixture);

    fixture.set_agents(&[]);
    fixture.run(["install-hooks"]);
    removed(&fixture);

    fixture.set_agents(&[agent]);
    fixture.run(["install-hooks"]);
    installed(&fixture);
    fixture.run(["uninstall-hooks"]);
    removed(&fixture);
    assert_absent(fixture.home.join(INTEGRATIONS_DIR));
}

pub fn write(path: impl AsRef<Path>, contents: &str) {
    let path = path.as_ref();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

pub fn write_skill(directory: impl AsRef<Path>, name: &str, body: &str) {
    write(
        directory.as_ref().join(name).join("SKILL.md"),
        &format!("---\nname: {name}\ndescription: {name} fixture\n---\n{body}"),
    );
}

pub fn read(path: impl AsRef<Path>) -> String {
    std::fs::read_to_string(path).unwrap()
}

pub fn assert_link(path: impl AsRef<Path>, target: impl AsRef<Path>) {
    assert_eq!(
        std::fs::read_link(path.as_ref()).unwrap(),
        target.as_ref(),
        "unexpected target for {}",
        path.as_ref().display()
    );
}

pub fn assert_absent(path: impl AsRef<Path>) {
    let path = path.as_ref();
    assert!(
        matches!(std::fs::symlink_metadata(path), Err(error) if error.kind() == std::io::ErrorKind::NotFound),
        "{} still exists or cannot be inspected",
        path.display()
    );
}

// Some agent CLIs print discovery results to stderr even on success.
pub fn output(command: &mut Command) -> String {
    let assertion = command.assert().success();
    let result = assertion.get_output();
    format!(
        "{}{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    )
}

// Exercise each skill destination independently, without enabling the other one.
pub fn skill_fixtures() -> impl Iterator<Item = (Fixture, &'static str)> {
    [
        ("claude", CLAUDE_SKILLS),
        ("codex,gemini,pi", SHARED_SKILLS),
    ]
    .into_iter()
    .map(|(agents, directory)| {
        eprintln!("Skill destination: {directory}; enabled agents: {agents}");
        (Fixture::for_agents(agents), directory)
    })
}
