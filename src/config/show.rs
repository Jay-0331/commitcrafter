//! Rendering for `cc config show`.
//!
//! Walks a [`Loaded`] config and produces either:
//!
//! - **TOML** — the effective merged config, each leaf followed by a
//!   `# source: <label> [(<path>)]` comment so users can see where
//!   every value came from without grepping their dotfiles.
//! - **JSON** — a flat object keyed by dotted path, each leaf carrying
//!   `{ value, source[, path] }`. For scripting / piping into `jq`.
//!
//! Output is deterministic: TOML walks the schema in the order
//! `toml::Table` exposes (alphabetical for nested tables, since
//! `toml::Table` is a `BTreeMap`), and JSON keys are produced via a
//! `BTreeMap` for the same reason. Snapshot tests rely on this.
//!
//! Rendering is pure — it does no I/O. The caller (typically
//! `main::run`) is responsible for loading the [`Loaded`] config and
//! writing the result wherever it likes.

use std::fmt::Write;

use crate::error::{Error, Result};

use super::merge::Loaded;
use super::source::{Source, Sources};

/// Render `loaded.config` as annotated TOML.
///
/// Each leaf line ends with `# source: <label>`. File-backed sources
/// (global / repo) also include the path so the user can `$EDITOR` it
/// straight from the output.
pub fn render_toml(loaded: &Loaded) -> Result<String> {
    let value = toml::Value::try_from(&loaded.config)
        .map_err(|e| Error::Config(format!("serialize merged config: {e}")))?;

    let table = value
        .as_table()
        .ok_or_else(|| Error::Config("merged config is not a table".into()))?;

    let mut out = String::new();
    let mut first_block = true;
    render_table(table, "", &mut out, &loaded.sources, &mut first_block)?;
    Ok(out)
}

/// Render `loaded` as flat JSON: `{ "dotted.path": { value, source, path? }, ... }`.
pub fn render_json(loaded: &Loaded) -> Result<String> {
    let value = toml::Value::try_from(&loaded.config)
        .map_err(|e| Error::Config(format!("serialize merged config: {e}")))?;

    let mut entries: std::collections::BTreeMap<String, serde_json::Value> =
        std::collections::BTreeMap::new();
    collect_leaves(&value, "", &mut |path, leaf| {
        let source = loaded.sources.get(path);
        let mut entry = serde_json::Map::new();
        entry.insert("value".to_string(), toml_to_json(leaf));
        entry.insert(
            "source".to_string(),
            serde_json::Value::String(source.map(Source::label).unwrap_or("?").to_string()),
        );
        if let Some(p) = source.and_then(Source::path) {
            entry.insert(
                "path".to_string(),
                serde_json::Value::String(p.display().to_string()),
            );
        }
        entries.insert(path.to_string(), serde_json::Value::Object(entry));
    });

    serde_json::to_string_pretty(&entries).map_err(|e| Error::Config(e.to_string()))
}

fn render_table(
    table: &toml::Table,
    prefix: &str,
    out: &mut String,
    sources: &Sources,
    first_block: &mut bool,
) -> Result<()> {
    let mut leaves: Vec<(&String, &toml::Value)> = Vec::new();
    let mut sub_tables: Vec<(&String, &toml::Table)> = Vec::new();

    for (k, v) in table {
        match v {
            toml::Value::Table(t) => sub_tables.push((k, t)),
            _ => leaves.push((k, v)),
        }
    }

    // Emit the header for this block if it has leaves or if it has no
    // sub-tables (so empty leaf sections still show up explicitly).
    let needs_header = !prefix.is_empty() && (!leaves.is_empty() || sub_tables.is_empty());
    if needs_header {
        if !*first_block {
            out.push('\n');
        }
        writeln!(out, "[{prefix}]").map_err(fmt_err)?;
        *first_block = false;
    }

    for (k, v) in leaves {
        let path = join_path(prefix, k);
        let source_str = format_source(sources.get(&path));
        let value_str = format_value(v)?;
        writeln!(out, "{k} = {value_str}  {source_str}").map_err(fmt_err)?;
    }

    for (k, t) in sub_tables {
        let child_prefix = join_path(prefix, k);
        render_table(t, &child_prefix, out, sources, first_block)?;
    }

    Ok(())
}

fn format_value(v: &toml::Value) -> Result<String> {
    // Re-serialize a single value via toml so we get correct quoting
    // for strings, formatting for arrays, etc.
    let helper_table = {
        let mut t = toml::Table::new();
        t.insert("v".to_string(), v.clone());
        t
    };
    let text = toml::to_string(&helper_table).map_err(|e| Error::Config(e.to_string()))?;
    // `text` looks like `v = <value>\n`. Strip the prefix.
    let rendered = text
        .trim_end()
        .strip_prefix("v = ")
        .ok_or_else(|| Error::Config(format!("unexpected toml output: {text:?}")))?;
    Ok(rendered.to_string())
}

fn format_source(source: Option<&Source>) -> String {
    match source {
        None => "# source: unknown".into(),
        Some(s) => match s.path() {
            Some(p) => format!("# source: {} ({})", s.label(), p.display()),
            None => format!("# source: {}", s.label()),
        },
    }
}

fn collect_leaves<F: FnMut(&str, &toml::Value)>(value: &toml::Value, prefix: &str, f: &mut F) {
    match value {
        toml::Value::Table(t) => {
            for (k, v) in t {
                let path = join_path(prefix, k);
                collect_leaves(v, &path, f);
            }
        }
        _ => f(prefix, value),
    }
}

fn toml_to_json(value: &toml::Value) -> serde_json::Value {
    match value {
        toml::Value::String(s) => serde_json::Value::String(s.clone()),
        toml::Value::Integer(i) => serde_json::Value::Number((*i).into()),
        toml::Value::Float(f) => serde_json::Number::from_f64(*f)
            .map(serde_json::Value::Number)
            .unwrap_or(serde_json::Value::Null),
        toml::Value::Boolean(b) => serde_json::Value::Bool(*b),
        toml::Value::Datetime(dt) => serde_json::Value::String(dt.to_string()),
        toml::Value::Array(a) => serde_json::Value::Array(a.iter().map(toml_to_json).collect()),
        toml::Value::Table(t) => {
            let mut obj = serde_json::Map::new();
            for (k, v) in t {
                obj.insert(k.clone(), toml_to_json(v));
            }
            serde_json::Value::Object(obj)
        }
    }
}

fn join_path(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

fn fmt_err(_: std::fmt::Error) -> Error {
    Error::Config("write to string buffer failed".into())
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::config::Layered;

    #[test]
    fn default_render_emits_every_section_header() {
        let loaded = Layered::new().load().unwrap();
        let text = render_toml(&loaded).unwrap();

        for header in [
            "[provider]",
            "[providers.anthropic]",
            "[providers.openai]",
            "[providers.openrouter]",
            "[providers.ollama]",
            "[style]",
            "[style.custom]",
            "[learning]",
            "[git]",
            "[ui]",
            "[ui.custom]",
        ] {
            assert!(text.contains(header), "missing {header} in:\n{text}");
        }
    }

    #[test]
    fn default_render_marks_every_leaf_as_default() {
        let loaded = Layered::new().load().unwrap();
        let text = render_toml(&loaded).unwrap();
        // Every "  # source:" comment should say `default`.
        for line in text.lines() {
            if let Some(comment) = line.split("# source:").nth(1) {
                assert!(
                    comment.trim().starts_with("default"),
                    "non-default source on line: {line:?}",
                );
            }
        }
    }

    #[test]
    fn global_overrides_appear_with_file_path() {
        let global_path = PathBuf::from("/tmp/cc-global.toml");
        let value: toml::Value = toml::from_str(
            r#"
            [provider]
            default = "openai"
            "#,
        )
        .unwrap();
        let loaded = Layered::new()
            .with_global_value(global_path.clone(), value)
            .load()
            .unwrap();

        let text = render_toml(&loaded).unwrap();
        let needle = format!("global ({})", global_path.display());
        assert!(
            text.contains(&needle),
            "expected `{needle}` in rendered output:\n{text}",
        );
    }

    #[test]
    fn set_overrides_appear_with_set_label() {
        let loaded = Layered::new()
            .with_set_arg("style.format=gitmoji")
            .unwrap()
            .load()
            .unwrap();
        let text = render_toml(&loaded).unwrap();

        // Find the `format = ...` line under [style] and assert the
        // comment carries `--set`.
        let format_line = text
            .lines()
            .find(|l| l.trim_start().starts_with("format ="))
            .expect("[style].format line present");
        assert!(
            format_line.contains("# source: --set"),
            "format line did not show --set source: {format_line:?}",
        );
        assert!(format_line.contains("\"gitmoji\""));
    }

    #[test]
    fn render_toml_round_trip_drops_comments_and_parses() {
        // Strip comments and assert the remaining TOML re-parses into
        // the same Config — protects against accidentally emitting
        // invalid TOML.
        let loaded = Layered::new().load().unwrap();
        let text = render_toml(&loaded).unwrap();

        let stripped: String = text
            .lines()
            .map(|line| match line.find("# source:") {
                Some(idx) => line[..idx].trim_end().to_string(),
                None => line.to_string(),
            })
            .collect::<Vec<_>>()
            .join("\n");

        let parsed = super::super::schema::Config::from_toml_str(&stripped)
            .expect("stripped render parses as TOML");
        assert_eq!(parsed, loaded.config);
    }

    #[test]
    fn render_json_produces_object_per_leaf() {
        let loaded = Layered::new().load().unwrap();
        let text = render_json(&loaded).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let obj = parsed.as_object().expect("top level is an object");

        // Spot-check a few well-known paths.
        for path in [
            "provider.default",
            "providers.anthropic.model",
            "style.subject_max_len",
            "learning.scope",
            "ui.theme",
        ] {
            let entry = obj.get(path).unwrap_or_else(|| panic!("missing {path}"));
            assert_eq!(entry["source"].as_str(), Some("default"));
            assert!(entry.get("value").is_some());
        }
    }

    #[test]
    fn render_json_includes_path_for_file_sources() {
        let global_path = PathBuf::from("/tmp/json-global.toml");
        let value: toml::Value = toml::from_str(
            r#"
            [providers.openai]
            model = "gpt-from-global"
            "#,
        )
        .unwrap();
        let loaded = Layered::new()
            .with_global_value(global_path.clone(), value)
            .load()
            .unwrap();
        let text = render_json(&loaded).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();

        let entry = &parsed["providers.openai.model"];
        assert_eq!(entry["source"].as_str(), Some("global"));
        assert_eq!(
            entry["path"].as_str(),
            Some(global_path.display().to_string().as_str()),
        );
        assert_eq!(entry["value"].as_str(), Some("gpt-from-global"));
    }

    #[test]
    fn render_json_omits_path_for_non_file_sources() {
        let loaded = Layered::new()
            .with_set_arg("style.format=conventional")
            .unwrap()
            .load()
            .unwrap();
        let text = render_json(&loaded).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();

        let entry = &parsed["style.format"];
        assert_eq!(entry["source"].as_str(), Some("--set"));
        assert!(
            entry.get("path").is_none(),
            "Set source should not include a path field; got: {entry:?}",
        );
    }

    #[test]
    fn json_output_is_alphabetically_sorted() {
        let loaded = Layered::new().load().unwrap();
        let text = render_json(&loaded).unwrap();
        // serde_json::to_string_pretty preserves BTreeMap order.
        // Walk paths in the order they appear and assert sorted.
        let mut last: Option<&str> = None;
        for line in text.lines() {
            // Lines of interest look like `  "path.here": {`.
            let line = line.trim();
            if !line.starts_with('"') || !line.contains(':') {
                continue;
            }
            let path = line.split('"').nth(1).expect("quoted path");
            // Skip nested keys (`"value":`, `"source":`).
            if !path.contains('.') && !["value", "source", "path"].contains(&path) {
                continue;
            }
            if !path.contains('.') {
                continue;
            }
            if let Some(prev) = last {
                assert!(
                    prev < path,
                    "JSON paths not sorted: {prev:?} came before {path:?}",
                );
            }
            last = Some(path);
        }
    }

    #[test]
    fn quoted_string_values_render_correctly() {
        let loaded = Layered::new()
            .with_set_arg(r#"provider.default="custom""#)
            .unwrap()
            .load()
            .unwrap();
        let text = render_toml(&loaded).unwrap();
        assert!(
            text.lines().any(|l| l.starts_with("default = \"custom\"")),
            "missing `default = \"custom\"` in:\n{text}",
        );
    }

    #[test]
    fn array_values_render_correctly() {
        let loaded = Layered::new().load().unwrap();
        let text = render_toml(&loaded).unwrap();
        // `[git].ignore_paths` is an array in defaults.
        let line = text
            .lines()
            .find(|l| l.trim_start().starts_with("ignore_paths"))
            .expect("ignore_paths line present");
        assert!(
            line.contains('['),
            "array value not rendered as TOML array: {line:?}",
        );
    }
}
