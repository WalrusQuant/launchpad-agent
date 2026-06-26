# CLI Resume Flags — `--continue` / `--resume` / `--session-id`

**Status: Phase 1 SHIPPED (2026-06-26). Phase 2 (resume flags) is next.** Parity roadmap §1 + §16.

Phase 1 routed all headless runs through the server (persisted) and — to avoid
regressing the existing headless flags — folded in the de-risked Phase 3
server-side env honoring. Verified end-to-end: full server round-trip
(spawn → initialize → session/start → turn/start → event-drain → completion),
rollout persisted (SessionMeta + Turn + Item lines), exit codes 0/1 correct,
final assistant text captured from item events. 467 tests pass (was 463; +3
tool-filter, +3 system-prompt, +2 csv-parse, −4 relocated from cli, +overlap).

## Goal

Bring headless (`-p`) up to Claude Code parity for session continuity:

```
lpagent -p "start a refactor plan"        # -> session abc123, persisted
lpagent -p --resume abc123 "apply step 1" # -> continues abc123, persisted
lpagent -p -c "and step 2"                # -> resumes most-recent session in cwd
lpagent -p --session-id <uuid> "..."      # -> run under a caller-chosen id (resume-or-create)
```

## Locked decisions

- **Architecture: headless drives the existing `StdioServerClient`** — the same path the TUI uses (`session_resume` / `session_list` / `session_start` / `turn_start` + notification draining). The server already does rollout load/replay/persist. Do **not** re-implement rollout replay in the CLI (the duplication the roadmap warns against).
- **Every headless run persists** (user decision 2026-06-26). Plain `-p` routes through the server too, so headless runs are themselves resumable/chainable. Accepts that headless now spawns the server subprocess (the TUI already does this on every launch).
- **Final assistant text comes from item events, not `turn/completed`.** `turn/completed` carries only status + usage. Mirror the worker's `latest_completed_agent_message` pattern: capture the agent message from item notifications, print it on completion.
- `--continue` = most-recently-`updated_at` session whose `cwd` == current dir.
- `--session-id` = resume if it exists, else start a new session with that id. Conflicts with `--resume`/`--continue` (clap `conflicts_with`).

## Phases

### Phase 1 — headless server-client driver
- [x] Extract headless into `crates/cli/src/headless.rs` (main.rs is ~670 lines; don't grow it). Move `HeadlessOptions`, `run_headless`, `apply_system_prompt_overrides`, `apply_tool_filters` + their tests.
- [x] Spawn the server via `StdioServerClient::spawn`, reusing `server_env_overrides` (lift it out of `agent.rs` to a shared spot, or duplicate the small builder). `initialize()` first.
- [x] Drive a turn: resolve `SessionId` (per flag, Phase 2) → `turn_start { session_id, prompt }` → drain notifications until `turn/completed` (exit 0) or `turn/failed` (exit 1). Capture final agent text from item events; preserve current contract: final assistant text → stdout, diagnostics → stderr.
- [x] Map turn status → documented exit codes (0/1).

### Phase 2 — flag resolution
- [ ] Add clap flags (global): `-r/--resume <SESSION_ID>`, `-c/--continue`, `--session-id <UUID>`; `conflicts_with` between the three.
- [ ] `--resume <id>`: parse id → `session_resume`; clean error (exit 1) on unknown id.
- [ ] `--continue`: `session_list` → filter `cwd == current_dir` → max `updated_at`; error (exit 1) if none.
- [ ] `--session-id <uuid>`: add `session_id: Option<SessionId>` to `SessionStartParams` (protocol) + handler honors it (resume-or-create-with-id). Validate uuid.
- [ ] No-flag path: `session_start` (persisted).

### Phase 3 — flag parity through the server path (DE-RISKED 2026-06-26 — no protocol changes)

**Finding:** because every headless run spawns its own private, single-tenant server subprocess, all flags cross the process boundary as **server env vars honored at bootstrap** — the same channel `server_env_overrides` (agent.rs) already uses for provider/model. **No `turn_start`/`session_start`/wire-protocol changes needed.** Two clean single-point insertion sites confirmed.

- [x] `--model` → existing `session_start.model` (or `LPA_MODEL`). No new work.
- [x] `--dangerously-skip-permissions` → existing `session_start` params: `permission_mode = "auto-approve"` + `sandbox_mode = "unrestricted"`. No new work.
- [x] `--allowed-tools` / `--disallowed-tools` → new env (`LPA_ALLOWED_TOOLS` / `LPA_DISALLOWED_TOOLS`), applied in `bootstrap.rs` right after `register_builtin_tools` + MCP registration via `registry.retain(...)`. **Move `apply_tool_filters` from `cli/main.rs` into `lpa-tools`** (or a shared spot) so CLI and server share one impl instead of duplicating.
- [x] `--system-prompt` / `--append-system-prompt` → new env (`LPA_SYSTEM_PROMPT` / `LPA_APPEND_SYSTEM_PROMPT`), stored on `ServerRuntimeDependencies` at bootstrap, applied to `model.base_instructions` in `execution.rs::resolve_turn_model` (single function, covers all 3 model-resolution branches). Reuse `apply_system_prompt_overrides`.

**Correctness caveat (documented, not a blocker):** env-based system-prompt/tool-filter overrides are *process-global*. That is exactly right for a single-tenant headless server, but would be wrong for a shared multi-session server. If these ever become per-session features (needed for subagents §6), they graduate to real protocol params then. Not needed now.

### Phase 4 — tests + docs
- [ ] Unit: flag parsing, conflict rejection, `--continue` selection (most-recent-in-cwd), uuid validation.
- [ ] Integration: `-p` start → persist → `-p --resume` round-trip shows continued context.
- [ ] Docs: README headless section, `docs/parity-roadmap.md` (§1 `--continue`/`--resume`/`--session-id` ✅, §16 ✅), CLAUDE.md headless bullet, exit-code docs. Bump test count.

## Open questions / risks
- ~~Phase 3 flag plumbing~~ — **RESOLVED**: env-var-at-bootstrap, no protocol changes (see Phase 3).
- `server_env_overrides` currently lives in `agent.rs`; sharing it cleanly may want a small `cli` helper module. Headless adds the 4 new env vars (`LPA_SYSTEM_PROMPT`, `LPA_APPEND_SYSTEM_PROMPT`, `LPA_ALLOWED_TOOLS`, `LPA_DISALLOWED_TOOLS`) to this builder.
- Confirm `session/start` handler path is reachable with a pre-chosen id without breaking the rollout filename scheme (`rollout-<ts>-<id>.jsonl`). This is the one remaining `--session-id` unknown (Phase 2).
- Final-text capture: `turn/completed` carries only status+usage, so the driver must accumulate agent text from `ItemDelta`/agent-message item events and flush on completion (mirror `latest_completed_agent_message`).

---

# Onboarding UX Overhaul

Currently `lpagent onboard` shows a model picker (builtin models only) and then asks "base url" / "api key" as free-text prompts. There's no concept of a provider preset, no connection validation, no "OpenRouter" / "Groq" / "Ollama" out-of-box, no masked API key, no way to see current config before changing it.

Target: **drop-in flow** where a user picks "OpenRouter" from a preset list, enters an API key, picks a free model from a curated list, and we verify the connection before saving.

## Locked decisions

- **Provider presets as first-class entries** alongside builtin models (anthropic/openai/google stay first; new: openrouter, groq, together, ollama, custom).
- **Preset knows base_url + wire_api + curated models**, so URL entry is skipped for presets.
- **Connection validation** (hits `/v1/models` or equivalent) runs after API key entry with a spinner; failure allows retry or skip-and-save.
- **Reconfigure via `/onboard`** (not a new `/settings`) — existing command gets a "current config" summary card at the top so reconfigure feels natural.
- **Masked API key display** in the summary (`sk-or-v1-****abcd` — last 4 chars only).
- Minimal changes to the selection.rs state machine — add new steps, don't rewrite.
- Keep builtin model picker flow intact for people who already know what they want.

## Scope — v1 MVP includes

- 7 provider presets (anthropic, openai, google, openrouter, groq, together, ollama, custom)
- Curated model list per preset (at least 3 models each, flagged "free" where applicable)
- Base URL auto-filled from preset (user can still override)
- API key masked in all rendering; entered via normal input (paste works via existing InputBuffer)
- Connection validation after key entry — green check or "try again / skip"
- Current config summary at start of `/onboard` showing provider, model, base URL, masked key
- Per-preset API key env-var hint ("We'll also read `OPENROUTER_API_KEY` from the environment")

## Scope — v1 MVP does NOT include

- Separate `/settings` command (reuse `/onboard`)
- Model list fetched live from provider API (use hardcoded curated list; fetch-on-demand is a follow-up)
- Multi-provider simultaneous config (one active provider at a time, same as today)
- OAuth / device-flow auth
- Ollama auto-discovery / status ping
- MCP server configuration inside onboarding (separate wishlist item)

---

## Phase A — Provider presets catalog

- [ ] New module `crates/core/src/provider_presets.rs` — `ProviderPreset { id, display_name, wire_api, default_base_url: Option<String>, api_key_env_vars: Vec<&str>, recommended_models: Vec<PresetModel> }`
- [ ] `PresetModel { slug, display_name, description: Option<String>, is_free: bool }`
- [ ] Seeded with 7 presets: Anthropic, OpenAI, Google, OpenRouter, Groq, Together AI, Ollama, Custom (sentinel)
- [ ] Curated models per preset — at least 3 each; include `:free` OpenRouter models
- [ ] Re-export from `lpa-core` lib root
- [ ] 3 unit tests: catalog_contains_expected_presets, openrouter_has_free_models, custom_preset_has_no_defaults

## Phase B — Onboarding uses presets

- [ ] `crates/tui/src/app.rs` — new onboarding state fields: `onboarding_preset: Option<&'static ProviderPreset>`, `onboarding_step: OnboardingStep` (`PickProvider | EnterApiKey | PickModel | Validating | Done`)
- [ ] `crates/tui/src/selection.rs` — replace `start_onboarding` first step with a **provider picker panel** showing presets; preset ID routes to matching builtin flow for anthropic/openai/google (keeps existing tests happy) or the new preset flow for openrouter/groq/etc
- [ ] When a preset has `default_base_url`, skip the base URL prompt and preload its URL into `onboarding_selected_base_url`
- [ ] After API key entry, show recommended models from the preset (not the full catalog)
- [ ] Tests: `onboarding_openrouter_flow_skips_base_url_prompt`, `onboarding_preset_shows_recommended_models`

## Phase C — Connection validation

**Shipped 2026-04-21 as a trimmed version.** The `ProviderValidationOutcome` struct and `Validating` step enum were dropped as dead weight (nothing produced `models_available`, and onboarding state is still tracked via booleans rather than an enum — a bigger refactor than the user benefit justifies). The failure-retry UX gap is closed.

- [x] Retry decision panel on validation failure — R/S/C/Esc intercept in `handle_key` mirrors the `pending_approval` pattern
- [x] `pending_validation_retry: Option<PendingValidationRetry>` on `TuiApp`; `onboarding_selected_*` preserved across the decision
- [x] Skip-and-save path → `finish_onboarding_selection()` with a transcript warning
- [x] Change path re-prompts for model (preset flow) or API key (legacy flow)
- [x] Test: `validation_failure_allows_retry_without_losing_input`
- [x] Plus: `validation_failure_enters_retry_state`, `validation_skip_pushes_save_without_probe_notice`, `validation_change_reprompts_for_model_in_preset_flow`

## Phase D — Current config summary card

- [ ] At the start of `/onboard`, render a card above the provider picker showing:
    - Current provider (display name)
    - Current model
    - Base URL (or "default")
    - API key: `***{last 4 chars}` or "not set"
    - Last validated at (if we persist validation timestamps — optional)
- [ ] Helper `fn summarize_current_config(&TuiApp) -> ConfigSummary`
- [ ] Test: `summary_masks_api_key_to_last_four_chars`

## Phase E — Polish

- [ ] Prompts use preset-aware labels: "OpenRouter API Key" instead of "api key"
- [ ] Env-var hint: "We'll also read `OPENROUTER_API_KEY` from the environment if set"
- [ ] Step indicator in prompt: `Step 2 of 4 — API key`
- [ ] Tests pass: `cargo test -p lpa-tui`, full workspace suite
- [ ] Manual smoke: `lpagent onboard` → pick OpenRouter → enter key → pick free model → actually send a prompt

---

## Risks / watch-outs

- **selection.rs already at 939 lines** (oversize per CLAUDE.md). Preset logic should land in a new submodule if it grows past ~100 lines.
- **Don't regress existing Anthropic/OpenAI/Google flows** — 17 existing onboarding tests in `crates/tui/src/tests.rs`. Run them between each phase.
- **Provider validation call shape** — need to check what `validate_provider_connection` currently expects and whether it handles OpenAI-compatible endpoints generically (likely yes via LPA_BASE_URL path).
- **Hardcoded model lists rot.** OpenRouter adds/removes free models often. Acceptable for v1; fetch-on-demand is the follow-up.

---

## Review

**2026-04-18: All 6 phases shipped.** Binary builds clean in release mode.

**Test delta:** 405 → 417 (+12 new). All green.

**What landed:**
- `crates/core/src/provider_presets.rs` — 9-preset catalog (Anthropic, OpenAI, Google, OpenRouter, Groq, Together, Mistral, Ollama, Custom) with wire API, default base URL, env-var fallbacks. 5 unit tests.
- `crates/tui/src/slash.rs` — `/configure` is the canonical command.
- `crates/tui/src/selection.rs` — `/onboard` kept as alias. New preset picker as first step. `handle_preset_selected` routes:
    - Anthropic/OpenAI/Google → existing builtin model catalog
    - Custom → legacy base-URL-first flow
    - Any other aggregator → base URL from preset, prompt for API key → prompt for model slug → validate
    - Ollama → no key, straight to model slug
- `crates/tui/src/runtime.rs` — `handle_submission` handles the new preset-driven state machine alongside the legacy one. `begin_onboarding_validation` helper deduplicates the validation trigger.
- `crates/tui/src/app.rs` — `AuxPanelContent::PresetList` variant, `PresetListEntry` struct.
- `crates/tui/src/render/mod.rs` — preset picker rendering + `preset_items` + `preset_panel_height` + `inline_preset_panel_height`.
- `crates/tui/src/worker_events.rs` — `mask_with_suffix` for last-4-chars display.
- Current-config summary pushed as `System` transcript item at start of `/configure`.

**Env var hint shown per preset:** "OpenRouter API key (also read from $OPENROUTER_API_KEY)".

**Key flow for testing OpenRouter today:**
1. Run `lpagent` (or `/configure` inside the TUI)
2. Pick "OpenRouter" from preset picker
3. Enter API key (or leave blank if `OPENROUTER_API_KEY` env var set)
4. Enter model slug (e.g. `meta-llama/llama-3.3-70b-instruct:free`)
5. Validation runs, on success the config is saved

**Known follow-ups (non-blocking):**
- `is_preset_picker_open` carries a `#[allow(dead_code)]` warning — safe to leave; used by tests and a reasonable public accessor.
- No "retry / skip validation" UI when validation fails — today it pushes an error and leaves the user in a partial state. Adding a dedicated Validating step with spinner + retry is the deferred Phase E.
- Hardcoded model lists per provider would be a nice next step once we pick which providers deserve curated entries.
- Preset render height for inline (non-onboarding) mode doesn't currently factor the title — minor visual.

**What's explicitly NOT here:**
- Connection validation UX (deferred Phase E — validation still happens, just without a polished retry flow)
- Live provider model list fetching
- MCP server configuration in onboarding
