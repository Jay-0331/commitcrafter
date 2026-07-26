//! End-to-end smoke tests for the default generate flow, one per v0.1
//! flag, driven through the offline mock provider.
//!
//! Each test builds a throwaway git repo, stages a change, sets
//! `COMMET_MOCK_RESPONSE` (so `provider::registry` returns the
//! mock), runs `cc`, and asserts the observable: stdout, the created
//! commit, or the prompt recorded to `COMMET_MOCK_LOG`.
//!
//! Compiled only with the `mock` feature (CI runs `--all-features`);
//! without it the whole file is empty.
#![cfg(feature = "mock")]

use std::fs;
use std::path::Path;
use std::process::Command;

use assert_cmd::Command as AssertCommand;
use tempfile::TempDir;

#[cfg(unix)]
use std::path::PathBuf;

/// A throwaway git repo with committer identity and signing disabled.
fn repo() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path();
    git(p, &["init", "-q"]);
    git(p, &["config", "user.email", "t@example.com"]);
    git(p, &["config", "user.name", "Tester"]);
    git(p, &["config", "commit.gpgsign", "false"]);
    dir
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?} failed");
}

/// Write and stage a file.
fn stage(dir: &Path, name: &str, contents: &str) {
    fs::write(dir.join(name), contents).unwrap();
    git(dir, &["add", name]);
}

/// A `cc` command rooted in `dir` with the mock response set. Learning
/// is turned off by default so a stray `-y` in these tests never writes
/// to the developer's real global history store; the recording test
/// re-enables it with a repo scope confined to its tempdir.
fn cc(dir: &Path, response: &str) -> AssertCommand {
    let mut cmd = AssertCommand::cargo_bin("commet").unwrap();
    cmd.current_dir(dir);
    cmd.env("COMMET_MOCK_RESPONSE", response);
    cmd.args(["--set", "learning.scope=off"]);
    cmd
}

/// Subject line of HEAD, or `None` when there are no commits yet.
fn head_subject(dir: &Path) -> Option<String> {
    let out = Command::new("git")
        .current_dir(dir)
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn staged_names(dir: &Path) -> Vec<String> {
    let output = Command::new("git")
        .current_dir(dir)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .unwrap();
    assert!(output.status.success(), "git diff --cached failed");
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[cfg(unix)]
fn real_git_binary() -> PathBuf {
    let path = std::env::var_os("PATH").expect("test process should have PATH");
    std::env::split_paths(&path)
        .map(|dir| dir.join("git"))
        .find(|candidate| candidate.is_file())
        .and_then(|candidate| candidate.canonicalize().ok())
        .expect("git should be available on PATH")
}

#[cfg(unix)]
fn install_git_argv_capture() -> (TempDir, std::ffi::OsString, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let shim_dir = tempfile::tempdir().unwrap();
    let shim = shim_dir.path().join("git");
    fs::write(
        &shim,
        r#"#!/bin/sh
if [ "$1" = "commit" ]; then
    : > "$COMMET_GIT_ARGV_LOG"
    for arg in "$@"; do
        printf '%s\n' "$arg" >> "$COMMET_GIT_ARGV_LOG"
    done
fi
exec "$COMMET_REAL_GIT" "$@"
"#,
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o755)).unwrap();

    let original_path = std::env::var_os("PATH").expect("test process should have PATH");
    let shim_path = std::env::join_paths(
        std::iter::once(shim_dir.path().to_path_buf()).chain(std::env::split_paths(&original_path)),
    )
    .unwrap();
    let argv_log = shim_dir.path().join("commit-argv");
    (shim_dir, shim_path, argv_log)
}

#[cfg(unix)]
fn captured_argv(log: &Path) -> Vec<String> {
    fs::read_to_string(log)
        .expect("git shim should capture the commit invocation")
        .lines()
        .map(str::to_string)
        .collect()
}

/// Read the JSON request the mock recorded at `log`.
fn logged_request(log: &Path) -> serde_json::Value {
    let raw = fs::read_to_string(log).unwrap();
    serde_json::from_str(&raw).unwrap()
}

#[test]
fn print_outputs_only_the_message_without_side_effects() {
    let _clipboard_guard = CLIPBOARD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut clipboard_state = ClipboardRestore::capture().and_then(|restore| {
        let mut clipboard = arboard::Clipboard::new().ok()?;
        clipboard.set_text("commet print sentinel").ok()?;
        Some((clipboard, restore))
    });

    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");

    cc(dir.path(), "feat: add a.txt")
        .arg("--print")
        .assert()
        .success()
        .stdout("feat: add a.txt\n")
        .stderr("");

    assert_eq!(head_subject(dir.path()), None, "--print must not commit");
    assert_eq!(staged_names(dir.path()), ["a.txt"]);
    if let Some((clipboard, _restore)) = clipboard_state.as_mut() {
        assert_eq!(clipboard.get_text().unwrap(), "commet print sentinel");
    }
}

#[test]
fn yes_commits_the_message() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");

    cc(dir.path(), "feat: add greeting")
        .arg("-y")
        .assert()
        .success();

    assert_eq!(
        head_subject(dir.path()).as_deref(),
        Some("feat: add greeting")
    );
}

#[test]
fn yes_with_g2_commits_the_first_candidate() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");

    cc(dir.path(), "feat: first\nfeat: second")
        .args(["-y", "-g", "2"])
        .assert()
        .success();

    // `-y` accepts the first candidate.
    assert_eq!(head_subject(dir.path()).as_deref(), Some("feat: first"));
}

#[test]
fn generate_flag_reaches_provider_and_prints_three_candidates() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");
    let log = dir.path().join("req.json");

    cc(dir.path(), "one\ntwo\nthree")
        .args(["-g", "3", "--print"])
        .env("COMMET_MOCK_LOG", &log)
        .assert()
        .success()
        .stdout("one\n\ntwo\n\nthree\n")
        .stderr("");

    assert_eq!(logged_request(&log)["n"], 3);
    assert_eq!(head_subject(dir.path()), None);
    assert_eq!(staged_names(dir.path()), ["a.txt"]);
}

#[test]
fn configured_generate_count_is_used_without_the_flag() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");
    let log = dir.path().join("req.json");

    cc(dir.path(), "first\nsecond")
        .args(["--set", "style.generate=2", "--print"])
        .env("COMMET_MOCK_LOG", &log)
        .assert()
        .success()
        .stdout("first\n\nsecond\n")
        .stderr("");

    assert_eq!(logged_request(&log)["n"], 2);
    assert_eq!(head_subject(dir.path()), None);
    assert_eq!(staged_names(dir.path()), ["a.txt"]);
}

#[test]
fn type_gitmoji_puts_the_rule_in_the_prompt() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");
    let log = dir.path().join("req.json");

    cc(dir.path(), "✨ add greeting")
        .args(["-t", "gitmoji", "--print"])
        .env("COMMET_MOCK_LOG", &log)
        .assert()
        .success();

    let req = logged_request(&log);
    assert!(
        req["system_prompt"].as_str().unwrap().contains("gitmoji"),
        "system prompt should carry the gitmoji rule"
    );
}

#[test]
fn type_custom_uses_configured_prompt_and_template_for_one_run() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");
    let config = r#"
[style]
format = "plain"

[style.custom]
system_prompt = "CUSTOM RELEASE RULES"
template = "<type>: <summary>\n\n<body>"
"#;
    fs::write(dir.path().join(".commet.toml"), config).unwrap();

    let custom_log = dir.path().join("custom.json");
    cc(dir.path(), "release: custom")
        .args(["-t", "custom", "--print"])
        .env("COMMET_MOCK_LOG", &custom_log)
        .assert()
        .success();

    assert_eq!(
        logged_request(&custom_log)["system_prompt"],
        "CUSTOM RELEASE RULES\n\nOutput template (follow exactly):\n<type>: <summary>\n\n<body>"
    );

    let plain_log = dir.path().join("plain.json");
    cc(dir.path(), "plain summary")
        .arg("--print")
        .env("COMMET_MOCK_LOG", &plain_log)
        .assert()
        .success();

    let plain_request = logged_request(&plain_log);
    let plain = plain_request["system_prompt"].as_str().unwrap();
    assert!(plain.contains("single concise sentence"));
    assert!(!plain.contains("CUSTOM RELEASE RULES"));
    assert_eq!(
        fs::read_to_string(dir.path().join(".commet.toml")).unwrap(),
        config
    );
}

#[test]
fn prompt_flag_appends_user_override() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");
    let config = r#"
[style]
extra_prompt = "include ticket id"
"#;
    fs::write(dir.path().join(".commet.toml"), config).unwrap();
    let log = dir.path().join("req.json");

    cc(dir.path(), "feat: saludo")
        .args(["--prompt", "write in Spanish", "--print"])
        .env("COMMET_MOCK_LOG", &log)
        .assert()
        .success();

    let request = logged_request(&log);
    let system = request["system_prompt"].as_str().unwrap();
    assert!(system.ends_with("--- USER OVERRIDE ---\ninclude ticket id\n\nwrite in Spanish"));
    assert_eq!(system.matches("--- USER OVERRIDE ---").count(), 1);

    let user = request["user_prompt"].as_str().unwrap();
    assert!(user.contains("hello"));
    assert!(!user.contains("include ticket id"));
    assert!(!user.contains("write in Spanish"));
    assert!(!user.contains("USER OVERRIDE"));
}

#[test]
fn exclude_merges_cli_and_config_filters_without_unstaging() {
    let dir = repo();
    stage(dir.path(), "keep.txt", "keep me\n");
    stage(dir.path(), "secret.env", "TOKEN=drop me\n");
    stage(dir.path(), "generated.rs", "generated cli exclusion\n");
    stage(dir.path(), "Cargo.lock", "configured exclusion\n");
    let log = dir.path().join("req.json");

    cc(dir.path(), "chore: update")
        .args(["-x", "*.env", "-x", "generated.*", "--print"])
        .env("COMMET_MOCK_LOG", &log)
        .assert()
        .success();

    let user = logged_request(&log)["user_prompt"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        user.contains("keep.txt"),
        "kept path should reach the prompt"
    );
    assert!(
        user.contains("keep me"),
        "kept diff should reach the prompt"
    );
    for excluded in [
        "secret.env",
        "TOKEN=drop me",
        "generated.rs",
        "generated cli exclusion",
        "Cargo.lock",
        "configured exclusion",
    ] {
        assert!(
            !user.contains(excluded),
            "excluded prompt data leaked: {excluded}\n{user}"
        );
    }

    assert_eq!(
        staged_names(dir.path()),
        ["Cargo.lock", "generated.rs", "keep.txt", "secret.env"],
        "exclude must not change the git index"
    );
}

#[cfg(unix)]
#[test]
fn no_verify_only_changes_the_git_commit_argv() {
    let dir = repo();
    let real_git = real_git_binary();
    let (_shim_dir, shim_path, argv_log) = install_git_argv_capture();

    stage(dir.path(), "plain.txt", "plain commit\n");
    cc(dir.path(), "feat: plain commit")
        .arg("-y")
        .env("PATH", &shim_path)
        .env("COMMET_REAL_GIT", &real_git)
        .env("COMMET_GIT_ARGV_LOG", &argv_log)
        .assert()
        .success();

    let plain_argv = captured_argv(&argv_log);
    assert_eq!(&plain_argv[..2], ["commit", "-F"]);
    assert_eq!(plain_argv.len(), 3, "unexpected argv: {plain_argv:?}");

    stage(dir.path(), "forced.txt", "skip hooks\n");
    let request_log = dir.path().join("no-verify-request.json");
    cc(dir.path(), "feat: skip hooks")
        .args(["-y", "--no-verify"])
        .env("PATH", &shim_path)
        .env("COMMET_REAL_GIT", &real_git)
        .env("COMMET_GIT_ARGV_LOG", &argv_log)
        .env("COMMET_MOCK_LOG", &request_log)
        .assert()
        .success();

    let no_verify_argv = captured_argv(&argv_log);
    assert_eq!(
        &no_verify_argv[..3],
        ["commit", "--no-verify", "-F"],
        "unexpected argv: {no_verify_argv:?}"
    );
    assert_eq!(
        no_verify_argv.len(),
        4,
        "unexpected argv: {no_verify_argv:?}"
    );

    let request = logged_request(&request_log);
    let system = request["system_prompt"].as_str().unwrap();
    let user = request["user_prompt"].as_str().unwrap();
    assert!(user.contains("forced.txt"));
    assert!(user.contains("skip hooks"));
    assert!(!system.contains("no-verify"));
    assert!(!user.contains("no-verify"));
    assert!(staged_names(dir.path()).is_empty());
    assert_eq!(
        head_subject(dir.path()).as_deref(),
        Some("feat: skip hooks")
    );
}

#[test]
fn yes_records_the_accepted_commit_to_the_repo_store() {
    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");

    // Override the helper's `scope=off` with a repo scope — the store
    // then lives inside this tempdir, never the real global path.
    cc(dir.path(), "feat: recorded")
        .args(["-y", "--set", "learning.scope=repo"])
        .assert()
        .success();

    let store = dir.path().join(".commet/history.jsonl");
    let content = fs::read_to_string(&store).expect("history file written");
    let rec: serde_json::Value = serde_json::from_str(content.lines().next().unwrap()).unwrap();

    assert_eq!(rec["edited_text"], "feat: recorded");
    assert_eq!(rec["accepted_index"], 0);
    assert!(
        rec["ts"].as_str().unwrap().ends_with('Z'),
        "ts is ISO-8601 UTC"
    );
    assert!(
        rec["files"]
            .as_array()
            .unwrap()
            .iter()
            .any(|f| f == "a.txt")
    );
}

static CLIPBOARD_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct ClipboardRestore {
    previous: Option<String>,
}

impl ClipboardRestore {
    fn capture() -> Option<Self> {
        let mut clipboard = arboard::Clipboard::new().ok()?;
        Some(Self {
            previous: clipboard.get_text().ok(),
        })
    }
}

impl Drop for ClipboardRestore {
    fn drop(&mut self) {
        let Ok(mut clipboard) = arboard::Clipboard::new() else {
            return;
        };
        if let Some(previous) = self.previous.take() {
            let _ = clipboard.set_text(previous);
        } else {
            let _ = clipboard.clear();
        }
    }
}

#[test]
fn clipboard_with_multiple_candidates_copies_first_headlessly_without_committing() {
    let _clipboard_guard = CLIPBOARD_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let Some(_restore) = ClipboardRestore::capture() else {
        assert!(
            std::env::var_os("CI").is_none(),
            "CI must provide a clipboard display"
        );
        eprintln!("skipping clipboard assertion: no display is available");
        return;
    };

    let dir = repo();
    stage(dir.path(), "a.txt", "hello\n");
    cc(dir.path(), "feat: copied\nfix: second\ndocs: third")
        .args(["-c", "-g", "3"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Copied: feat: copied"));

    let mut clipboard = arboard::Clipboard::new().expect("clipboard remains available");
    assert_eq!(clipboard.get_text().unwrap(), "feat: copied");
    assert_eq!(head_subject(dir.path()), None);
}
