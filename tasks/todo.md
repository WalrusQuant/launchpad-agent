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

- [ ] `crates/tui/src/worker.rs` — extend `validate_provider` / `validate_provider_connection` to take the full config tuple and return structured `ProviderValidationOutcome { success, message, models_available: Option<Vec<String>> }`
- [ ] `crates/tui/src/selection.rs` — add `Validating` step; show spinner; on success transition to `Done` + save; on failure show retry/skip prompt
- [ ] Fall back: "skip validation and save anyway" option, for local/stub endpoints
- [ ] Test: `validation_failure_allows_retry_without_losing_input`

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
