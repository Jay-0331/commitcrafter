//! Public-API coverage for learning-store persistence, selection, and privacy.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

#[cfg(feature = "mock")]
use std::process::Command;

#[cfg(feature = "mock")]
use assert_cmd::Command as AssertCommand;
use commet::config::LearningScope;
use commet::learning::{LearningRecord, MAX_BYTES, Store, append, load};
use tempfile::tempdir;

fn record(ts: &str, format: &str, text: &str) -> LearningRecord {
    LearningRecord {
        ts: ts.into(),
        repo: "commet".into(),
        branch: "main".into(),
        provider: "anthropic".into(),
        model: "claude-sonnet-4-6".into(),
        format: format.into(),
        candidates: vec![text.into()],
        accepted_index: 0,
        edited_text: text.into(),
        files: vec!["src/lib.rs".into()],
        diff_bytes: 42,
        diff: "diff --git a/src/lib.rs b/src/lib.rs".into(),
    }
}

fn archive_path(base: &Path, number: u32) -> PathBuf {
    let mut name = base.as_os_str().to_owned();
    name.push(format!(".{number}"));
    PathBuf::from(name)
}

fn make_oversized(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(path)
        .unwrap();
    file.set_len(MAX_BYTES + 1).unwrap();
}

#[test]
fn append_and_load_round_trip_preserves_the_record() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.jsonl");
    let expected = record("2026-08-05T12:00:00Z", "conventional", "feat: persist");

    append(&path, &expected).unwrap();

    assert_eq!(load(&path).unwrap(), vec![expected]);
}

#[test]
fn examples_are_newest_first_and_deduplicated_by_edited_text() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.jsonl");
    let store = Store::with_paths(LearningScope::Repo, Some(path), None);

    for entry in [
        record("2026-08-05T12:00:00Z", "conventional", "feat: shared"),
        record("2026-08-05T12:01:00Z", "conventional", "fix: middle"),
        record("2026-08-05T12:02:00Z", "gitmoji", "✨ ignored"),
        record("2026-08-05T12:03:00Z", "conventional", "feat: shared"),
        record("2026-08-05T12:04:00Z", "conventional", "chore: newest"),
    ] {
        store.write(&entry).unwrap();
    }

    assert_eq!(
        store.load_examples("conventional", 10).unwrap(),
        ["chore: newest", "feat: shared", "fix: middle"]
    );
}

#[test]
fn scope_filters_reads_for_off_repo_global_and_repo_global() {
    let dir = tempdir().unwrap();
    let repo_path = dir.path().join("repo/history.jsonl");
    let global_path = dir.path().join("global/history.jsonl");
    append(
        &repo_path,
        &record("2026-08-05T12:00:00Z", "conventional", "repo entry"),
    )
    .unwrap();
    append(
        &global_path,
        &record("2026-08-05T12:01:00Z", "conventional", "global entry"),
    )
    .unwrap();

    for (scope, expected) in [
        (LearningScope::Off, Vec::<&str>::new()),
        (LearningScope::Repo, vec!["repo entry"]),
        (LearningScope::Global, vec!["global entry"]),
        (
            LearningScope::RepoGlobal,
            vec!["repo entry", "global entry"],
        ),
    ] {
        let store = Store::with_paths(scope, Some(repo_path.clone()), Some(global_path.clone()));
        let texts: Vec<_> = store
            .read()
            .unwrap()
            .into_iter()
            .map(|entry| entry.edited_text)
            .collect();
        assert_eq!(texts, expected, "unexpected records for {scope:?}");
    }
}

#[test]
fn scope_filters_writes_for_off_repo_global_and_repo_global() {
    for (scope, repo_written, global_written) in [
        (LearningScope::Off, false, false),
        (LearningScope::Repo, true, false),
        (LearningScope::Global, false, true),
        (LearningScope::RepoGlobal, true, true),
    ] {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().join("repo/history.jsonl");
        let global_path = dir.path().join("global/history.jsonl");
        let store = Store::with_paths(scope, Some(repo_path.clone()), Some(global_path.clone()));

        store
            .write(&record(
                "2026-08-05T12:00:00Z",
                "conventional",
                "scoped entry",
            ))
            .unwrap();

        assert_eq!(repo_path.exists(), repo_written, "repo path for {scope:?}");
        assert_eq!(
            global_path.exists(),
            global_written,
            "global path for {scope:?}"
        );
    }
}

#[test]
fn appending_to_an_oversized_store_rotates_it_to_dot_one() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.jsonl");
    make_oversized(&path);

    let fresh = record("2026-08-05T12:00:00Z", "conventional", "fresh live entry");
    append(&path, &fresh).unwrap();

    assert_eq!(
        fs::metadata(archive_path(&path, 1)).unwrap().len(),
        MAX_BYTES + 1
    );
    assert_eq!(load(&path).unwrap(), vec![fresh]);
}

#[test]
fn rotation_keeps_three_archives_and_never_creates_dot_four() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("history.jsonl");

    for generation in 1..=4 {
        make_oversized(&path);
        append(
            &path,
            &record(
                &format!("2026-08-05T12:0{generation}:00Z"),
                "conventional",
                &format!("generation {generation}"),
            ),
        )
        .unwrap();
    }

    assert!(archive_path(&path, 1).exists());
    assert!(archive_path(&path, 2).exists());
    assert!(archive_path(&path, 3).exists());
    assert!(!archive_path(&path, 4).exists());
}

#[cfg(feature = "mock")]
#[test]
fn store_diffs_false_never_serializes_raw_diff_content() {
    const SECRET: &str = "PRIVATE_DIFF_SENTINEL_7f30d3";

    let dir = tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "-q"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("private.txt"), format!("{SECRET}\n")).unwrap();
    git(repo, &["add", "private.txt"]);

    let mut command = AssertCommand::cargo_bin("commet").unwrap();
    command
        .current_dir(repo)
        .env("COMMET_MOCK_RESPONSE", "test: keep the diff private")
        .args([
            "--yes",
            "--set",
            "learning.scope=repo",
            "--set",
            "learning.store_diffs=false",
        ])
        .assert()
        .success();

    let raw = fs::read_to_string(repo.join(".commet/history.jsonl")).unwrap();
    let stored: LearningRecord = serde_json::from_str(raw.lines().next().unwrap()).unwrap();
    assert!(
        stored.diff_bytes > 0,
        "diff metadata should still be retained"
    );
    assert_eq!(stored.diff, "");
    assert!(
        !raw.contains(SECRET),
        "raw diff content leaked into history"
    );
}

#[cfg(feature = "mock")]
fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}
