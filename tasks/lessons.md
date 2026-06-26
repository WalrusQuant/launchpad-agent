# Lessons

## When a deleted file/config reappears, find what WRITES it — don't just delete again

**Context (2026-06-25):** User's real `~/.launchpad/agent/config.toml` kept showing
junk (`model = "test-model"`, provider `anthropic`, `api_key = "sk-test"`) in the
`/configure` flow. I deleted the file and called it fixed. It came back minutes
later and the user was (rightly) angry: "you didn't fix shit."

**Root cause:** TUI tests (`crates/tui/src/tests.rs`) exercise the save path
(`skip_validation_and_save` → `save_onboarding_config` → `find_lpa_home()`), and
`find_lpa_home()` falls back to the REAL `~/.launchpad/agent/` when `LPA_HOME` is
unset. The tests never isolated `LPA_HOME`, so every `cargo test --workspace` —
including the ones I ran while "helping" — overwrote the user's real config with
the `test-model`/`sk-test` fixture.

**Rule for myself:**
- If a file/config reappears after deletion, treat deletion as a symptom patch and
  immediately hunt for the writer (grep the literal values, check timestamps against
  what I just ran). Timestamps are a strong signal — the junk reappeared right after
  my `cargo test` run.
- Tests that hit `find_lpa_home()`/`$HOME` paths must redirect to a temp dir. The fix
  here was a process-global `override_lpa_home()` in `lpa-utils` (no env mutation,
  since `std::env::set_var` is unsound under multi-threaded test runs).
- Don't declare "fixed" on a stateful symptom without proving the state can't be
  recreated. I proved it by deleting the config, running the suite, and confirming
  the real file stayed gone (junk landed in `$TMPDIR/lpa-tui-test-home/`).

## z.ai / GLM provider facts (verified against docs.z.ai 2026-06-25)

- Auth is standard `Authorization: Bearer <key>` (OpenAI-compatible). The lpagent
  OpenAI provider already sends this **only when api_key is Some** — a 401
  "Authentication parameter not received in Header" means the key was None, not wrong.
- Base URL: general `https://api.z.ai/api/paas/v4`, GLM **coding plan**
  `https://api.z.ai/api/coding/paas/v4`. Current flagship model slug: `glm-5.2`.
- Onboarding 401 path: validation uses `onboarding_selected_api_key`; if the user
  doesn't paste a key at the prompt it stays None → 401. A directly-written
  `config.toml` (key inline) bypasses the onboarding flow entirely.
