use crate::common::{
    Fixture, GLOBAL_INSTRUCTIONS, INTEGRATIONS_DIR, SHARED_SKILLS, assert_absent, assert_link,
    check_hooks, read, write,
};

const INSTRUCTIONS: &str = ".pi/agent/AGENTS.md";
const AGENT: &str = "pi";
const EXTENSION_LINK: &str = ".pi/agent/extensions/agentlink.ts";
const ADAPTER: &str = "plugins/agentlink/adapters/pi.ts";

#[test]
#[ignore = "requires agent CLIs"]
fn hook_lifecycle() {
    check_hooks(AGENT, assert_installed, assert_removed);
}

fn assert_installed(f: &Fixture) {
    assert_link(
        f.home.join(EXTENSION_LINK),
        f.home.join(INTEGRATIONS_DIR).join(ADAPTER),
    );
    f.command(AGENT)
        .env("PI_OFFLINE", "1")
        .arg("--list-models")
        .assert()
        .success();
}

fn assert_removed(f: &Fixture) {
    assert_absent(f.home.join(EXTENSION_LINK));
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
