//! Acceptance coverage for config-layer precedence and source tracking.
//!
//! These tests intentionally load real files from `tests/fixtures/config` so
//! parsing, deep merge, typed deserialization, diagnostics, and source labels
//! are exercised together rather than only through in-memory TOML values.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use commet::config::{Config, Layered, Loaded, MessageFormat, Source};
use commet::error::Error;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/config")
        .join(name)
}

fn source_snapshot(loaded: &Loaded, paths: &[&str]) -> String {
    paths
        .iter()
        .map(|path| {
            let source = loaded
                .sources
                .get(path)
                .unwrap_or_else(|| panic!("missing source for {path}"));
            match source.path() {
                Some(file) => format!(
                    "{path} = {} ({})",
                    source.label(),
                    file.file_name().unwrap().to_string_lossy()
                ),
                None => format!("{path} = {}", source.label()),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn defaults_only_uses_the_typed_defaults_and_default_sources() {
    let loaded = Layered::new().load().unwrap();

    assert_eq!(loaded.config, Config::default());
    assert!(!loaded.sources.is_empty());
    for (path, source) in loaded.sources.iter() {
        assert_eq!(source, &Source::Default, "unexpected source for {path}");
    }
}

#[test]
fn global_only_overrides_mentioned_leaves() {
    let global = fixture("global.toml");
    let loaded = Layered::new()
        .with_global_file(&global)
        .unwrap()
        .load()
        .unwrap();

    assert_eq!(loaded.config.provider.default, "openai");
    assert_eq!(loaded.config.providers.anthropic.model, "global-model");
    assert_eq!(loaded.config.providers.anthropic.max_tokens, 2048);
    assert_eq!(loaded.config.style.subject_max_len, 80);
    assert_eq!(
        loaded.sources.get("providers.anthropic.model"),
        Some(&Source::Global(global))
    );
    assert_eq!(loaded.sources.get("learning.scope"), Some(&Source::Default));
}

#[test]
fn repo_only_overrides_mentioned_leaves() {
    let repo = fixture("repo.toml");
    let loaded = Layered::new()
        .with_repo_file(&repo)
        .unwrap()
        .load()
        .unwrap();

    assert_eq!(loaded.config.providers.anthropic.model, "repo-model");
    assert_eq!(loaded.config.style.format, MessageFormat::Gitmoji);
    assert_eq!(
        loaded.config.providers.anthropic.max_tokens,
        Config::default().providers.anthropic.max_tokens
    );
    assert_eq!(
        loaded.sources.get("style.format"),
        Some(&Source::Repo(repo))
    );
}

#[test]
fn repo_resolves_conflicts_while_global_supplies_other_values() {
    let global = fixture("global.toml");
    let repo = fixture("repo.toml");
    let loaded = Layered::new()
        .with_global_file(&global)
        .unwrap()
        .with_repo_file(&repo)
        .unwrap()
        .load()
        .unwrap();

    assert_eq!(loaded.config.providers.anthropic.model, "repo-model");
    assert_eq!(loaded.config.providers.anthropic.max_tokens, 2048);
    assert_eq!(
        loaded.sources.get("providers.anthropic.model"),
        Some(&Source::Repo(repo))
    );
    assert_eq!(
        loaded.sources.get("providers.anthropic.max_tokens"),
        Some(&Source::Global(global))
    );
}

#[test]
fn set_override_wins_and_source_snapshot_is_stable() {
    let loaded = Layered::new()
        .with_global_file(fixture("global.toml"))
        .unwrap()
        .with_repo_file(fixture("repo.toml"))
        .unwrap()
        .with_set_arg("style.subject_max_len=44")
        .unwrap()
        .load()
        .unwrap();

    assert_eq!(loaded.config.style.subject_max_len, 44);
    assert_eq!(
        source_snapshot(
            &loaded,
            &[
                "learning.scope",
                "provider.default",
                "providers.anthropic.max_tokens",
                "providers.anthropic.model",
                "style.format",
                "style.subject_max_len",
            ]
        ),
        "learning.scope = default\n\
provider.default = global (global.toml)\n\
providers.anthropic.max_tokens = global (global.toml)\n\
providers.anthropic.model = repo (repo.toml)\n\
style.format = repo (repo.toml)\n\
style.subject_max_len = --set"
    );
}

#[test]
fn unknown_file_keys_emit_warnings_and_known_keys_still_load() {
    let tmp = tempfile::tempdir().unwrap();
    let config_home = tmp.path().join("config-home");
    let commet_dir = config_home.join("commet");
    fs::create_dir_all(&commet_dir).unwrap();
    fs::copy(fixture("unknown.toml"), commet_dir.join("config.toml")).unwrap();

    let output = Command::cargo_bin("commet")
        .unwrap()
        .current_dir(tmp.path())
        .env("HOME", tmp.path())
        .env("XDG_CONFIG_HOME", &config_home)
        .env("COMMET_LOG", "warn")
        .args(["config", "show"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("default = \"ollama\""));

    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.matches("unknown config key (ignored)").count(), 2);
    assert!(stderr.contains("provider.mystery"), "warning: {stderr}");
    assert!(stderr.contains("experimental"), "warning: {stderr}");
    assert!(stderr.contains("config.toml"), "warning: {stderr}");
}

#[test]
fn invalid_toml_is_a_config_error_with_path_line_and_source_text() {
    let invalid = fixture("invalid.toml");
    let error = Layered::new().with_global_file(&invalid).unwrap_err();

    let Error::Config(message) = error else {
        panic!("expected Config error, got {error:?}");
    };
    assert!(message.contains("invalid.toml"), "diagnostic: {message}");
    assert!(message.contains("line 3"), "diagnostic: {message}");
    assert!(
        message.contains("include_body = ["),
        "diagnostic: {message}"
    );
}
