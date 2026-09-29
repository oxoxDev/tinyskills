//! Opt-in collision policies.

use std::fs;
use std::path::{Path, PathBuf};

use tinyskills::{
    CollisionPolicy, DiscoveryRoot, Skill, SkillScope, TieBreak, discover, discover_with,
    resolve_collisions, resolve_collisions_with,
};

fn skill(name: &str, dir: &str, scope: SkillScope, location: &str) -> Skill {
    Skill {
        name: name.to_owned(),
        dir_name: dir.to_owned(),
        scope,
        location: Some(PathBuf::from(location)),
        ..Skill::default()
    }
}

fn last_wins() -> CollisionPolicy {
    CollisionPolicy {
        tie_break: TieBreak::LastWins,
        excluded_scopes: vec![SkillScope::Profile],
        id_noun: "workflow".to_owned(),
    }
}

#[test]
fn default_policy_matches_resolve_collisions() {
    let skills = || {
        vec![
            skill("a", "a", SkillScope::User, "/z/a/SKILL.md"),
            skill("a", "a", SkillScope::User, "/b/a/SKILL.md"),
        ]
    };
    let plain = resolve_collisions(skills());
    let policy = resolve_collisions_with(skills(), &CollisionPolicy::default());
    assert_eq!(plain.len(), 1);
    assert_eq!(plain[0].location, policy[0].location);
    assert_eq!(plain[0].warnings, policy[0].warnings);
    assert_eq!(plain[0].location, Some(PathBuf::from("/b/a/SKILL.md")));
    assert!(plain[0].warnings[0].starts_with("shadowed User-scope skill 'a' (skill id 'a')"));
}

#[test]
fn last_wins_lets_the_incoming_skill_win_equal_precedence() {
    let found = resolve_collisions_with(
        [
            skill("a", "a", SkillScope::User, "/b/a/SKILL.md"),
            skill("a", "a", SkillScope::User, "/z/a/SKILL.md"),
        ],
        &last_wins(),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].location, Some(PathBuf::from("/z/a/SKILL.md")));
    assert_eq!(
        found[0].warnings,
        ["shadowed User-scope skill 'a' (workflow id 'a') at /b/a/SKILL.md"]
    );
}

#[test]
fn last_wins_still_honors_scope_precedence() {
    let found = resolve_collisions_with(
        [
            skill("a", "a", SkillScope::Project, "/p/a/SKILL.md"),
            skill("a", "a", SkillScope::User, "/u/a/SKILL.md"),
        ],
        &last_wins(),
    );
    assert_eq!(found[0].scope, SkillScope::Project);
    assert_eq!(
        found[0].warnings,
        ["workflow id 'a' or name 'a' also declared in User scope at /u/a/SKILL.md (ignored)"]
    );
}

#[test]
fn directory_id_collisions_apply_across_names() {
    let found = resolve_collisions_with(
        [
            skill("first", "shared", SkillScope::User, "/1/shared/SKILL.md"),
            skill("second", "shared", SkillScope::User, "/2/shared/SKILL.md"),
        ],
        &last_wins(),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "second");
}

#[test]
fn excluded_scopes_are_dropped() {
    let found = resolve_collisions_with(
        [
            skill("a", "a", SkillScope::Profile, "/p/a/SKILL.md"),
            skill("b", "b", SkillScope::User, "/u/b/SKILL.md"),
        ],
        &last_wins(),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].name, "b");
    assert_eq!(
        resolve_collisions([skill("a", "a", SkillScope::Profile, "/p")]).len(),
        1
    );
}

fn write_skill(dir: &Path, description: &str) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: shared\ndescription: {description}\n---\n"),
    )
}

#[test]
fn root_order_is_the_tie_break_under_last_wins() -> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    write_skill(&temp.path().join("early/shared"), "early")?;
    write_skill(&temp.path().join("late/shared"), "late")?;
    let root = |name: &str| DiscoveryRoot::new(temp.path().join(name), SkillScope::User);

    let forward = discover_with([root("early"), root("late")], &last_wins());
    assert_eq!(forward[0].description, "late");
    let reverse = discover_with([root("late"), root("early")], &last_wins());
    assert_eq!(reverse[0].description, "early");

    // The default stays order-independent: the smaller location wins.
    let stable_a = discover([root("early"), root("late")]);
    let stable_b = discover([root("late"), root("early")]);
    assert_eq!(stable_a[0].description, "early");
    assert_eq!(stable_b[0].description, "early");
    Ok(())
}
