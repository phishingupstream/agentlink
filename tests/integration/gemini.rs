use crate::common::{
    Fixture, GLOBAL_INSTRUCTIONS, INTEGRATIONS_DIR, PLUGIN_NAME, SHARED_SKILLS, assert_absent,
    assert_link, check_hooks, output, read, write,
};

const INSTRUCTIONS: &str = ".gemini/GEMINI.md";
const AGENT: &str = "gemini";
const EXTENSION_LINK: &str = ".gemini/extensions/agentlink";
const PLUGIN_DIR: &str = "plugins/agentlink";
const TRUST_WORKSPACE_ENV: &str = "GEMINI_CLI_TRUST_WORKSPACE";

#[test]
#[ignore = "requires agent CLIs"]
fn hook_lifecycle() {
    check_hooks(AGENT, assert_installed, assert_removed);
}

fn assert_installed(f: &Fixture) {
    assert_link(
        f.home.join(EXTENSION_LINK),
        f.home.join(INTEGRATIONS_DIR).join(PLUGIN_DIR),
    );
    let listed = output(f.command(AGENT).args(["extensions", "list"]));
    assert!(listed.contains(PLUGIN_NAME), "{listed}");
}

fn assert_removed(f: &Fixture) {
    assert_absent(f.home.join(EXTENSION_LINK));
}

fn listed_skills(f: &Fixture) -> String {
    output(
        f.command(AGENT)
            .env(TRUST_WORKSPACE_ENV, "true")
            .args(["skills", "list", "--all"]),
    )
}

#[test]
#[ignore = "requires agent CLIs"]
fn discovers_user_skills() {
    let f = Fixture::for_agents(AGENT);
    f.run(["use", "alpha", "--user"]);

    let listed = listed_skills(&f);

    assert!(listed.contains("alpha"), "{listed}");
}

#[test]
#[ignore = "requires agent CLIs"]
fn discovers_project_skills() {
    let f = Fixture::for_agents(AGENT);
    f.run(["use", "alpha", "beta"]);

    let listed = listed_skills(&f);

    assert!(
        listed.contains("alpha") && listed.contains("beta"),
        "{listed}"
    );
}

#[test]
#[ignore = "requires agent CLIs"]
fn retired_skills_disappear_from_discovery() {
    let f = Fixture::for_agents(AGENT);
    f.run(["use", "alpha", "beta"]);
    let before = listed_skills(&f);
    assert!(before.contains("beta"), "{before}");
    f.run_git(&f.source, ["rm", "--quiet", "-r", "skills/beta"]);
    f.push("remove selected skill");

    f.run(["update"]);

    let listed = listed_skills(&f);
    assert!(!listed.contains("beta"), "{listed}");
    assert!(listed.contains("alpha"), "{listed}");
}

#[test]
fn instruction_links_follow_registry_changes() {
    let f = Fixture::for_agents(AGENT);
    assert_link(
        f.home.join(INSTRUCTIONS),
        f.registry.join(GLOBAL_INSTRUCTIONS),
    );
    write(f.source.join(GLOBAL_INSTRUCTIONS), "global-v2\n");
    f.push("update instructions");

    f.run(["update"]);

    assert_eq!(read(f.home.join(INSTRUCTIONS)), "global-v2\n");
    std::fs::remove_file(f.source.join(GLOBAL_INSTRUCTIONS)).unwrap();
    f.push("remove instructions");
    f.run(["update"]);
    f.run(["sync"]);
    assert_absent(f.home.join(INSTRUCTIONS));
}

#[test]
fn paths_follow_enabled_agent_selection() {
    let f = Fixture::for_agents(AGENT);
    f.run(["use", "alpha", "--user"]);
    f.run(["use", "alpha"]);
    f.assert_skill_links(&f.home, SHARED_SKILLS, &["alpha"]);
    f.assert_skill_links(&f.project, SHARED_SKILLS, &["alpha"]);
    f.set_agents(&[]);

    f.run(["sync"]);

    assert_absent(f.home.join(INSTRUCTIONS));
    f.assert_no_skill_links(&f.home, &["alpha"]);
    f.assert_no_skill_links(&f.project, &["alpha"]);
}
