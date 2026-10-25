use crate::common::{
    CLAUDE_SKILLS, Fixture, GLOBAL_INSTRUCTIONS, PLUGIN_ID, PLUGIN_NAME, assert_absent,
    assert_link, check_hooks, read, write,
};
use predicates::prelude::*;

const INSTRUCTIONS: &str = ".claude/CLAUDE.md";
const AGENT: &str = "claude";

#[test]
#[ignore = "requires agent CLIs"]
fn hook_lifecycle() {
    check_hooks(AGENT, assert_installed, assert_removed);
}

fn assert_installed(f: &Fixture) {
    f.command(AGENT)
        .args(["plugin", "marketplace", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(PLUGIN_NAME));
    f.command(AGENT)
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(PLUGIN_ID));
}

fn assert_removed(f: &Fixture) {
    f.command(AGENT)
        .args(["plugin", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(PLUGIN_ID).not());
    f.command(AGENT)
        .args(["plugin", "marketplace", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(PLUGIN_NAME).not());
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
    f.assert_skill_links(&f.home, CLAUDE_SKILLS, &["alpha"]);
    f.assert_skill_links(&f.project, CLAUDE_SKILLS, &["alpha"]);
    f.set_agents(&[]);

    f.run(["sync"]);

    assert_absent(f.home.join(INSTRUCTIONS));
    f.assert_no_skill_links(&f.home, &["alpha"]);
    f.assert_no_skill_links(&f.project, &["alpha"]);
}
