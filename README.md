# commitcrafter

AI-powered git commit message CLI with a `ratatui` TUI.

**Status:** planning — no code yet. Track the v0.1.0 MVP via the milestone
and epic issues in this repo.

## What it will do

- Inspect your working tree and let you pick which files to stage from a TUI.
- Send the staged diff to a configurable LLM provider
  (Anthropic Claude, OpenAI, OpenRouter, or local Ollama).
- Preview the generated message(s), let you accept / regenerate / edit in
  `$EDITOR`, or copy to clipboard.
- Learn from your accepted commits and feed them as few-shot examples on
  future runs.

## Roadmap

See the [v0.1.0 MVP milestone](../../milestones) and the `epic`-labeled
issues for the full plan. v0.1 ships:

- 4 provider adapters with a shared HTTP base layer.
- File-level staging picker.
- Multi-candidate generation (`-g N`).
- Per-run overrides for type, prompt, exclude globs, no-verify.
- Conventional / gitmoji / subject+body / plain formats.
- `cc setup` and `cc doctor` for first-run + health checks.
- Local learning store (per-repo + global), opt-out via config.

## Why

The "stare at diff, type a generic message" loop is slow and the resulting
messages are usually worse than what a model could produce in 2 seconds
from the same diff.

## License

MIT
