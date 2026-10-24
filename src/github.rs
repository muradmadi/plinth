use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::step::Step;

/// The repo plinth creates for a new project.
pub struct Repo {
    pub owner: String,
    pub name: String,
    pub description: String,
    pub public: bool,
}

impl Repo {
    pub fn slug(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// The labels every project uses. Issue templates apply `bug` and `feature`, Renovate applies
/// `dependencies`, and a PR gets the one matching its title's type.
const LABELS: &[(&str, &str, &str)] = &[
    ("feature", "2da44e", "Something new it can do"),
    ("bug", "d73a4a", "Something it does wrong"),
    ("docs", "0075ca", "README, AGENTS.md, docs/"),
    ("ci", "6e7781", "Workflows, repo settings, tooling"),
    ("dependencies", "0366d6", "Dependency updates (Renovate)"),
];

/// Protects the default branch: changes arrive only as a squash-merged PR whose `check` job passed.
/// Every language's CI must name its gate job `check`. The branch needn't be up to date with `main`:
/// release-please opens a PR after every merge, so requiring that would leave every other PR behind
/// each time. Personal repos can't use a merge queue, and CI on `main` catches what slips through.
const RULESET: &str = r#"{
  "name": "main",
  "target": "branch",
  "enforcement": "active",
  "conditions": {"ref_name": {"include": ["~DEFAULT_BRANCH"], "exclude": []}},
  "rules": [
    {"type": "deletion"},
    {"type": "non_fast_forward"},
    {"type": "required_linear_history"},
    {"type": "pull_request", "parameters": {
      "allowed_merge_methods": ["squash"],
      "dismiss_stale_reviews_on_push": false,
      "require_code_owner_review": false,
      "require_last_push_approval": false,
      "required_approving_review_count": 0,
      "required_review_thread_resolution": true
    }},
    {"type": "required_status_checks", "parameters": {
      "do_not_enforce_on_create": false,
      "strict_required_status_checks_policy": false,
      "required_status_checks": [{"context": "check", "integration_id": 15368}]
    }}
  ]
}
"#;

/// The account gh is logged in as, used when `--owner` isn't given.
pub fn logged_in_user() -> Result<String> {
    let out = Command::new("gh")
        .args(["api", "user", "--jq", ".login"])
        .output()
        .context("couldn't run gh; is it installed?")?;
    if !out.status.success() {
        bail!("gh isn't logged in; run `gh auth login` first");
    }
    Ok(String::from_utf8(out.stdout)?.trim().to_owned())
}

/// Fails if the repo already exists, before any file is written.
pub fn ensure_absent(repo: &Repo) -> Result<()> {
    let out = Command::new("gh")
        .args(["repo", "view", &repo.slug(), "--json", "name"])
        .output()
        .context("couldn't run gh; is it installed?")?;
    if out.status.success() {
        bail!("{} already exists on GitHub", repo.slug());
    }
    Ok(())
}

/// Fails if the project's git repo can't take plinth's commit and push: it already has a remote,
/// or it has commits and uncommitted changes that the commit would sweep in. A repo without
/// commits yet, as `cargo new` leaves it, is fine; its first commit is the whole project.
pub fn ensure_ready(root: &Path) -> Result<()> {
    if !root.join(".git").exists() {
        return Ok(());
    }
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .context("couldn't run git; is it installed?")
    };
    if git(&["remote", "get-url", "origin"])?.status.success() {
        bail!(
            "{} already has a git remote; plinth only creates new GitHub repos so far, so run it \
             with --no-github",
            root.display()
        );
    }
    let has_commits = git(&["rev-parse", "--verify", "--quiet", "HEAD"])?
        .status
        .success();
    if has_commits && !git(&["status", "--porcelain"])?.stdout.is_empty() {
        bail!(
            "{} has uncommitted changes; commit or stash them first, so plinth's commit holds only \
             its own files",
            root.display()
        );
    }
    Ok(())
}

/// The first commit, the repo, and the settings that make the PR workflow hold, in order. The
/// ruleset comes last because it blocks the first push to `main`.
pub fn steps(repo: &Repo) -> Vec<Step> {
    let slug = repo.slug();
    let api = format!("repos/{slug}");
    let mut steps = vec![
        Step::new(["git", "add", "--all"]),
        Step::new([
            "git",
            "commit",
            "--quiet",
            "--message",
            &format!("chore: set up {} with plinth", repo.name),
        ]),
    ];

    let mut create = vec![
        "gh".to_owned(),
        "repo".into(),
        "create".into(),
        slug.clone(),
        if repo.public { "--public" } else { "--private" }.into(),
        "--source".into(),
        ".".into(),
        "--remote".into(),
        "origin".into(),
        "--push".into(),
    ];
    if !repo.description.is_empty() {
        create.extend(["--description".into(), repo.description.clone()]);
    }
    steps.push(Step::new(create));

    steps.push(Step::new([
        "gh",
        "api",
        "--silent",
        "--method",
        "PATCH",
        &api,
        "-F",
        "allow_squash_merge=true",
        "-F",
        "allow_merge_commit=false",
        "-F",
        "allow_rebase_merge=false",
        "-f",
        "squash_merge_commit_title=PR_TITLE",
        "-f",
        "squash_merge_commit_message=BLANK",
        "-F",
        "delete_branch_on_merge=true",
        "-F",
        "allow_update_branch=true",
        "-F",
        "allow_auto_merge=true",
        "-F",
        "has_wiki=false",
        "-F",
        "has_projects=false",
    ]));
    // release-please opens its release PR with the job token, which needs this.
    steps.push(Step::new([
        "gh",
        "api",
        "--silent",
        "--method",
        "PUT",
        &format!("{api}/actions/permissions/workflow"),
        "-f",
        "default_workflow_permissions=read",
        "-F",
        "can_approve_pull_request_reviews=true",
    ]));

    for (name, color, description) in LABELS {
        steps.push(Step::new([
            "gh",
            "label",
            "create",
            name,
            "--repo",
            &slug,
            "--color",
            color,
            "--description",
            description,
            "--force",
        ]));
    }
    // GitHub's own starter labels change over time, so delete whatever isn't one of ours rather
    // than a fixed list.
    let keep: Vec<String> = LABELS
        .iter()
        .map(|(name, _, _)| format!("-e {name}"))
        .collect();
    steps.push(Step::new([
        "bash".to_owned(),
        "-c".into(),
        format!(
            "set -o pipefail; gh label list --repo {slug} --limit 100 --json name --jq \".[].name\" \
             | {{ grep -vxF {} || true; }} \
             | while IFS= read -r label; do gh label delete \"$label\" --repo {slug} --yes; done",
            keep.join(" ")
        ),
    ]));

    steps.push(
        Step::new([
            "gh",
            "api",
            "--silent",
            "--method",
            "POST",
            &format!("{api}/rulesets"),
            "--input",
            "-",
        ])
        .with_stdin(RULESET.to_owned()),
    );
    steps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo(public: bool, description: &str) -> Repo {
        Repo {
            owner: "someone".into(),
            name: "demo".into(),
            description: description.into(),
            public,
        }
    }

    fn lines(repo: &Repo) -> Vec<String> {
        steps(repo).iter().map(ToString::to_string).collect()
    }

    #[test]
    fn private_unless_asked() {
        assert!(
            lines(&repo(false, ""))
                .iter()
                .any(|l| l.starts_with("gh repo create someone/demo --private "))
        );
        assert!(
            lines(&repo(true, ""))
                .iter()
                .any(|l| l.starts_with("gh repo create someone/demo --public "))
        );
    }

    #[test]
    fn description_goes_to_github_only_when_given() {
        assert!(
            lines(&repo(false, "A demo"))
                .iter()
                .any(|l| l.ends_with("--description 'A demo'"))
        );
        assert!(
            !lines(&repo(false, ""))
                .iter()
                .any(|l| l.starts_with("gh repo create") && l.contains("--description"))
        );
    }

    #[test]
    fn commits_before_creating_and_protects_main_last() {
        let lines = lines(&repo(false, ""));
        let create = lines
            .iter()
            .position(|l| l.starts_with("gh repo create"))
            .unwrap();
        let commit = lines
            .iter()
            .position(|l| l.starts_with("git commit"))
            .unwrap();
        assert!(commit < create);
        assert!(lines.last().unwrap().contains("/rulesets"));
    }
}
