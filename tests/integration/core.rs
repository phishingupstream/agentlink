use crate::common::{
    LEGACY_SKILL_DIRS, PROJECT_CONFIG, USER_CONFIG, assert_absent, read, skill_fixtures, write,
    write_skill,
};
use predicates::prelude::*;

const LOCAL_FILE: &str = "local/SKILL.md";
const COLLISION_FILE: &str = "alpha/SKILL.md";
const LOCAL_CONTENT: &str = "local\n";
const USER_SKILLS_STATUS: &str = "user skills: alpha";
const USER_CURRENT_STATUS: &str = "user status: current";
const MIGRATED_SKILL: &str = "local-migration";
const MIGRATION_ORIGIN: &str = "ssh://git@example.invalid/apps/migration-project.git";

#[test]
fn user_selection_replaces_collisions_and_preserves_local_skills() {
    for (f, directory) in skill_fixtures() {
        write(f.home.join(directory).join(COLLISION_FILE), "collision\n");
        write(f.home.join(directory).join(LOCAL_FILE), LOCAL_CONTENT);

        f.run(["use", "alpha", "--user"]);

        assert_eq!(f.config().skills, ["alpha"]);
        f.assert_skill_links(&f.home, directory, &["alpha"]);
        assert_eq!(read(f.home.join(directory).join(LOCAL_FILE)), LOCAL_CONTENT);
    }
}

#[test]
fn project_selection_tracks_project_and_preserves_local_skills() {
    for (f, directory) in skill_fixtures() {
        write(
            f.project.join(directory).join(COLLISION_FILE),
            "collision\n",
        );
        write(f.project.join(directory).join(LOCAL_FILE), LOCAL_CONTENT);

        f.run(["use", "alpha", "beta"]);

        assert_eq!(f.manifest().skills, ["alpha", "beta"]);
        assert_eq!(
            f.config().projects.as_slice(),
            std::slice::from_ref(&f.project)
        );
        f.assert_skill_links(&f.project, directory, &["alpha", "beta"]);
        assert_eq!(
            read(f.project.join(directory).join(LOCAL_FILE)),
            LOCAL_CONTENT
        );
        for directory in LEGACY_SKILL_DIRS {
            for skill in ["alpha", "beta"] {
                assert_absent(f.project.join(directory).join(skill));
            }
        }
    }
}

#[test]
fn status_shows_user_selection_outside_enabled_project() {
    for (f, directory) in skill_fixtures() {
        f.run(["use", "alpha", "--user"]);
        f.assert_skill_links(&f.home, directory, &["alpha"]);

        f.run(["status"]).stdout(
            predicate::str::contains(USER_SKILLS_STATUS)
                .and(predicate::str::contains(USER_CURRENT_STATUS))
                .and(predicate::str::contains("status: not enabled")),
        );
    }
}

#[test]
fn status_shows_user_project_and_local_skills() {
    for (f, directory) in skill_fixtures() {
        f.run(["use", "alpha", "--user"]);
        f.run(["use", "alpha", "beta"]);
        write(f.project.join(directory).join(LOCAL_FILE), LOCAL_CONTENT);

        f.run(["status"]).stdout(
            predicate::str::contains(USER_SKILLS_STATUS)
                .and(predicate::str::contains(USER_CURRENT_STATUS))
                .and(predicate::str::contains("agentlink skills: alpha, beta"))
                .and(predicate::str::contains("\nstatus: current\n"))
                .and(predicate::str::contains("other skills: local")),
        );
    }
}

#[test]
fn removing_user_skill_preserves_project_selection() {
    for (f, directory) in skill_fixtures() {
        f.run(["use", "alpha", "--user"]);
        f.run(["use", "alpha"]);
        write(f.home.join(directory).join(LOCAL_FILE), LOCAL_CONTENT);

        f.run(["unuse", "alpha", "--user"]);

        assert!(f.config().skills.is_empty());
        assert_eq!(f.manifest().skills, ["alpha"]);
        f.assert_no_skill_links(&f.home, &["alpha"]);
        f.assert_skill_links(&f.project, directory, &["alpha"]);
        assert_eq!(read(f.home.join(directory).join(LOCAL_FILE)), LOCAL_CONTENT);
    }
}

#[test]
fn removing_user_skill_is_idempotent() {
    for (f, directory) in skill_fixtures() {
        f.run(["use", "alpha", "--user"]);
        f.run(["unuse", "alpha", "--user"]);
        let config = read(f.home.join(USER_CONFIG));

        f.run(["unuse", "alpha", "--user"]);

        assert_eq!(read(f.home.join(USER_CONFIG)), config);
        assert!(f.config().skills.is_empty());
        f.assert_no_skill_links(&f.home, &["alpha"]);
        assert_absent(f.home.join(directory).join("alpha"));
    }
}

#[test]
fn removing_project_skill_preserves_other_skills() {
    for (f, directory) in skill_fixtures() {
        f.run(["use", "alpha", "beta"]);
        write(f.project.join(directory).join(LOCAL_FILE), LOCAL_CONTENT);

        f.run(["unuse", "alpha"]);

        assert_eq!(f.manifest().skills, ["beta"]);
        f.assert_no_skill_links(&f.project, &["alpha"]);
        f.assert_skill_links(&f.project, directory, &["beta"]);
        assert_eq!(
            read(f.project.join(directory).join(LOCAL_FILE)),
            LOCAL_CONTENT
        );
    }
}

#[test]
fn migration_preserves_files_and_records_provenance() {
    for (f, directory) in skill_fixtures() {
        let apps = f.project.join("apps");
        let project = apps.join("migration-project");
        let reference = format!("{MIGRATED_SKILL}/references/example.txt");
        write_skill(project.join(directory), MIGRATED_SKILL, "");
        write(project.join(directory).join(&reference), "preserved\n");
        f.run_git(&project, ["init", "--quiet", "--initial-branch=main"]);
        f.run_git(&project, ["remote", "add", "origin", MIGRATION_ORIGIN]);
        f.agentlink()
            .arg("discover")
            .arg(&apps)
            .assert()
            .success()
            .stdout(predicate::str::contains(project.to_str().unwrap()).and(
                predicate::str::contains(format!("{MIGRATED_SKILL}: {directory}")),
            ));

        f.agentlink()
            .args(["migrate", MIGRATED_SKILL, "--project"])
            .arg(&project)
            .assert()
            .success();

        f.run_git(
            &f.source,
            ["pull", "--quiet", "--ff-only", "origin", "main"],
        );
        f.run_git(&f.source, ["log", "-1", "--format=%B"])
            .stdout(predicate::str::contains(format!(
                "Source: {MIGRATION_ORIGIN}"
            )));
        assert_eq!(f.manifest_at(&project).skills, [MIGRATED_SKILL]);
        assert_eq!(read(f.source.join("skills").join(reference)), "preserved\n");
        f.assert_skill_links(&project, directory, &[MIGRATED_SKILL]);
        f.agentlink()
            .arg("discover")
            .arg(&apps)
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "no unmanaged project-local skills found",
            ));
    }
}

#[test]
fn update_refreshes_selected_skills() {
    for (f, directory) in skill_fixtures() {
        f.run(["use", "beta", "--user"]);
        f.run(["use", "beta"]);
        write_skill(f.source.join("skills"), "beta", "beta-v2\n");
        f.push("update");

        f.run(["update"]);

        for base in [&f.home, &f.project] {
            assert!(
                read(base.join(directory).join("beta/SKILL.md"))
                    .lines()
                    .any(|line| line == "beta-v2")
            );
        }
    }
}

#[test]
fn retired_skill_is_removed_and_empty_project_is_untracked() {
    for (f, directory) in skill_fixtures() {
        for base in [&f.home, &f.project] {
            write(base.join(directory).join(LOCAL_FILE), LOCAL_CONTENT);
        }
        f.run(["use", "beta", "--user"]);
        f.run(["use", "beta"]);
        f.run_git(&f.source, ["rm", "--quiet", "-r", "skills/beta"]);
        f.push("remove selected skill");

        f.run(["update-if-due"]).stderr(
            predicate::str::contains("removed retired skill(s) `beta`")
                .and(predicate::str::contains(
                    "removed retired user skill(s) `beta`",
                ))
                .and(predicate::str::contains(
                    "review and commit the manifest change",
                )),
        );

        assert!(f.config().skills.is_empty());
        assert!(f.config().projects.is_empty());
        assert!(f.manifest().skills.is_empty());
        for base in [&f.home, &f.project] {
            f.assert_no_skill_links(base, &["beta"]);
            assert_eq!(read(base.join(directory).join(LOCAL_FILE)), LOCAL_CONTENT);
        }
        assert_absent(f.registry.join("skills/beta"));
    }
}

#[test]
fn untracking_preserves_manifest_and_removes_links() {
    for (f, directory) in skill_fixtures() {
        f.run(["use", "alpha"]);
        f.assert_skill_links(&f.project, directory, &["alpha"]);
        let manifest = read(f.project.join(PROJECT_CONFIG));

        f.run(["untrack"]);

        assert_eq!(read(f.project.join(PROJECT_CONFIG)), manifest);
        assert!(f.config().projects.is_empty());
        f.assert_no_skill_links(&f.project, &["alpha"]);
    }
}

// Requirement #1: local registry origins must survive changing working directories.
#[test]
fn relative_registry_path_updates_from_another_directory() {
    let f = crate::common::Fixture::new();
    f.run(["init", "../registry.git", "--agents", "codex"]);
    assert_eq!(std::path::Path::new(&f.config().registry), f.remote);
    f.agentlink()
        .current_dir(&f.home)
        .arg("update")
        .assert()
        .success();
    f.run(["use", "alpha"]);
    f.assert_skill_links(&f.project, crate::common::SHARED_SKILLS, &["alpha"]);
}
