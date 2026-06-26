# CLAUDE.md — Launchpad Agent

Instructions for Claude Code working in this repo. Read this before making changes.

---

## What this is

**Launchpad Agent** (`lpagent`) — a Rust-based coding agent, rebranded from an upstream project called ClawCR. Provider-agnostic (Anthropic / OpenAI / Google Gemini), client/server architecture, TUI as one of many possible clients.

Status: early, not production-ready. Single-author project.

## Workspace layout

12 crates, all prefixed `lpa-`:

| Crate | Purpose |
|-------|---------|
| `cli` | Entry point (`lpagent` binary); `onboard`, `prompt`, `doctor`, `server` subcommands |
| `core` | Query loop, session/turn/item model, config, token budgeting, skills, provider presets |
| `tools` | Tool impls (bash, read, grep, apply_patch, write, webfetch, websearch, skill, plan, todowrite, question, invalid) + orchestrator + registry + `McpToolAdapter`. `TaskTool` and `LspTool` are defined but **not registered** (stubs). |
| `provider` | Anthropic / OpenAI / Google provider SDKs behind `ModelProviderSDK` + `ProviderAdapter` |
| `safety` | Secret redaction, `PermissionPolicy` (Allow/Deny/Ask), approval cache, safety modes |
| `server` | WebSocket/JSON-RPC runtime, session persistence, approval manager, MCP bootstrap |
| `protocol` | JSON-RPC wire types, events, model catalog schema |
| `client` | stdio + WebSocket transports (used by TUI and future clients) |
| `tui` | Ratatui-based terminal UI, `/configure` flow, worker event loop, welcome banner |
| `mcp` | **v1 MVP shipped** — JSON-RPC 2.0, stdio transport, supervisor, `StdMcpManager`. HTTP transport gated behind `streamable-http` feature. |
| `tasks` | Task manager stubs |
| `utils` | `LPA_HOME` resolution, config paths |

## Build & test

No justfile / Makefile. Use cargo directly:

```bash
cargo build --release
cargo test --workspace      # 421 tests currently passing
cargo run -- onboard        # TUI configure flow (alias: the app launches straight into it on first run)
cargo run -- prompt "..."   # Single-shot completion
```

Rust 1.85+. All 421 tests currently pass — keep it that way.

## Config & env

- `LPA_HOME` — config dir (default `~/.launchpad/agent`)
- `LPA_PROVIDER`, `LPA_MODEL`, `LPA_WIRE_API`, `LPA_BASE_URL`, `LPA_API_KEY`
- Per-provider key fallbacks: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`, `GOOGLE_API_KEY` / `GEMINI_API_KEY`
- User config: `~/.launchpad/agent/config.toml`
- Project overrides: `<workspace>/.lpagent/config.toml`

## Request flow (where to start when debugging)

1. `crates/cli/src/main.rs` — command dispatch
2. `crates/server/src/runtime.rs` (180 lines) + `crates/server/src/runtime/` — JSON-RPC handlers, turn scheduling, approval manager. Already split into `runtime/{execute_turn,handlers_session,handlers_turn,...}.rs`.
3. `crates/core/src/query/mod.rs` (~570 lines) — streaming loop, tool-call handling, runaway guard, compaction trigger. Compaction helpers in `crates/core/src/query/compaction.rs`.
4. `crates/tools/src/orchestrator.rs` — permission check → tool dispatch; `Ask` decisions go through `ApprovalChannel`
5. `crates/provider/src/{anthropic,openai,google}/` — provider-specific wire format

## What's implemented vs. stubbed

**Works end-to-end:**
- Single- and multi-turn completions via any of the 3 native providers (Anthropic / OpenAI / Google) or any OpenAI-compatible endpoint (OpenRouter, Groq, Together, Mistral, Ollama, custom)
- Streaming, tool calls (bash, read, grep, apply_patch, write, glob, webfetch, websearch, skill, update_plan, todowrite, question, invalid)
- **MCP runtime v1** — stdio-transport servers declared in `[[mcp.servers]]` auto-discover their tools and register them as `mcp__<server>__<tool>` in the tool registry. Per-server `trust_level = "trusted"` pre-seeds the approval cache. See `wishlist.md` §1 for phase detail.
- **LLM-based context compaction** — selector → summarization → JSON snapshot → prompt-view rebuild → rollout journal. Falls back to legacy naive drop on summarizer failure. See `crates/core/src/compaction/`.
- **Approval flow (UI + cache)** — TUI prompts with y/n/Esc when an `Ask` decision arrives, sends the response via `approval/respond`, and renders the resolution in the transcript. `RuntimeSession` owns a shared `Arc<tokio::sync::Mutex<ApprovalCache>>` that the orchestrator consults — approving a tool at `Session` or `Tool` scope skips future asks for that tool for the rest of the session.
- **Session persistence** (rollout files in `~/.launchpad/agent/sessions/`)
- **Secret redaction**, permission policy (rule-based)
- **`/configure` onboarding flow** with 25 provider presets (anthropic, openai, google, openrouter, groq, together, mistral, deepseek, xai, fireworks, cerebras, perplexity, moonshot, deepinfra, nebius, hyperbolic, novita, sambanova, lambda, nvidia, github, zai_coding, ollama, lmstudio, custom). Each non-custom preset ships a **curated, selectable model list** (`PresetModel` in `provider_presets.rs`), so every provider — not just first-party — shows a model picker; a trailing "Custom model…" row drops to manual slug entry with a provider-specific `slug_hint` example. **API keys are reused automatically**: selecting a new model for a provider that already has a saved key skips the key prompt entirely (a validation failure re-prompts for the key). Current-config summary card, masked key display via `/config`, reasoning toggle via `/reasoning`. `/onboard` kept as an alias.
- **Polished TUI** — slate + cyan palette, ASCII logo banner, `❯` user messages with slate bubble, `◇ tool  args` / `└ preview` tree connectors for tool calls, collapsible reasoning (`∙ thinking…` → `∙ thought (N chars)`), explicit end-of-turn markers (`◼ interrupted`, `Max tokens reached`, `No response`).

**Stubbed or partial — do not assume these exist:**
- **TaskTool** — returns acknowledgment, not registered by default. Re-add to `register_builtin_tools` + `register_builtin_runtime_tools` when real subagent dispatch lands.
- **LspTool** — stub, not registered by default.
- **OS-level sandboxing** — no process isolation yet. BUT a rule-based workspace sandbox IS now enforced: set `[sandbox] enabled = true` (optional `workspace_write`) in config.toml and the query loop attaches a `RuleBasedPolicy::with_sandbox` that denies file writes outside cwd and (when `workspace_write = false`) all writes + shell. Sandbox denials override AutoApprove/Ask. See `crates/safety/src/legacy_permissions.rs` (`SandboxContext`, `evaluate_sandbox`) and `crates/core/src/config/safety.rs` (`SandboxConfig`). `SandboxMode::External` still unimplemented.
- **Skill hot-reload** — `watch_for_changes` config exists, no fs-events impl
- **Tool progress events** — types exist, not streamed end-to-end
- **MCP HTTP / Streamable-HTTP transport** — gated behind the `streamable-http` feature flag; types present, not implemented
- **MCP resources / prompts / sampling / elicitation** — out of scope for v1

See `wishlist.md` for the remaining roadmap.

See `wishlist.md` and `implementation-plan.md` for the current roadmap.

## House rules (from `AGENTS.md`)

- All internal crates use `lpa-` prefix.
- Inline `format!` args (`format!("{x}")`, not `format!("{}", x)`); prefer method refs over closures; collapse nested `if`.
- No `bool`/`Option` positional args that produce ambiguous callsites — use enums, newtypes, or named-arg `/*param*/` comments.
- Exhaustive `match` — no wildcards unless justified.
- New traits require doc comments explaining purpose and implementor contract.
- **Modules target <500 lines, hard guidance <800.** See "Known oversize files" below.
- Tests: use `pretty_assertions::assert_eq`, compare full objects, platform-aware paths (`#[cfg(unix)]`/`#[cfg(windows)]`), never mutate env vars.
- Don't introduce trivial single-use helper functions.

## Known oversize files (don't grow further; split when touching)

Sizes verified 2026-06. Several files the older notes flagged are already split
(server `runtime.rs` → `runtime/`, `query.rs` → `query/`, `apply_patch.rs` →
`apply_patch/`, `render/mod.rs`, `safety/lib.rs`, `tui/runtime.rs`). Still over
the 800-line guideline — adding new functionality should go into new sibling
modules:

- `crates/tui/src/tests.rs` (1,606) — test file; split into a `tests/` dir by domain when touched
- `crates/provider/src/openai/chat_completions.rs` (1,469) — could extract response parsing
- `crates/provider/src/openai/chat_completions/stream.rs` (1,125) — streaming state machine (largely irreducible)
- `crates/provider/src/anthropic/messages.rs` (1,105) — could split request build vs. response parse

Near the line (700–800, watch when editing): `apply_patch/tests.rs` (771),
`render/markdown.rs` (755), `openai/responses.rs` (750), `safety/lib.rs` (748),
`tui/worker/mod.rs` (733), `protocol/model.rs` (720), `render/mod.rs` (710),
`google/generate_content.rs` (687), `server/persistence.rs` (675).

The big provider wire-format files are large because of irreducible
wire-format complexity, not accidental coupling — split only if a clean seam
(request-build vs. response-parse) appears.

## Known issues / footguns

- **Provider request serialization `expect`** — `crates/provider/src/anthropic/messages.rs:658` does `serde_json::to_value(body).expect(...)`. Low risk (only fails on OOM of a struct we just built), but don't copy the pattern; return `Err` in new providers. (The older "6 panic sites on malformed responses" note was wrong — the other `expect!`/`panic!` are all in `#[cfg(test)]`.)
- **Tool input parsing uses `unwrap_or` defaults** for *optional* fields (e.g. `bash` timeout/tty), so wrong-typed JSON for those silently falls back to a safe default. Required fields ARE validated (`.ok_or_else`), and tool-call argument JSON that fails to parse is now surfaced to the model as a recoverable error tool_result rather than run as `{}` — see the tool-call assembly in `crates/core/src/query/mod.rs`.
- **Runaway guard:** the agent loop bails out after `MAX_AUTONOMOUS_STEPS` (1000) consecutive model calls without fresh user input (`crates/core/src/query/mod.rs`). Resets on new user input.
- **apply_patch path safety:** `resolve_relative` (`crates/tools/src/apply_patch/apply.rs`) resolves `.`/`..` lexically and rejects any reference that escapes the workspace root, independent of the sandbox.
- **Windows shell detection** — shell resolution lives in `crates/tools/src/shell_exec.rs` (`resolve_shell`); it handles cmd/powershell but Windows paths are lightly tested. Verify before relying on it.
- Rebrand from the upstream ClawCR project is complete across source, config, env vars, and docs. If you see stray `clawcr` / `ClawCodeRust` strings outside `target/` or historical notes, fix them.

## Working rules for Claude in this repo

1. **Plan before touching large files** (see "Known oversize files" — the biggest now are the provider wire-format parsers and `tui/src/tests.rs`). Ask whether to split before adding.
2. **Run `cargo test --workspace` after changes** — ~449 tests is the pass/fail signal. All green is the baseline. Also run `cargo fmt --all -- --check` (CI gates on rustfmt) and `cargo clippy` before pushing.
3. **Follow `AGENTS.md`** (clippy-style formatting, module size, no `bool` positional params). Don't restate its rules in code review; enforce them in diffs.
4. **Don't fabricate status** — if something is stubbed, say so. The wishlist/plan docs are the source of truth on what's "done".
5. **Keep user-facing docs (`README.md`, `docs/*.md`) in sync** when you change behavior. Docs have already drifted from code; don't widen the gap.
6. **Before marking a task complete**, prove it: run tests, show a diff, or demonstrate the behavior.
7. **Project-level task tracking** uses `tasks/todo.md` and `tasks/lessons.md` per the user's global conventions. `tasks/todo.md` exists — check for an in-progress plan before starting new non-trivial work.
