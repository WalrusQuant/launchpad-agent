# Launchpad Agent Build-Out Wishlist

## 1. MCP Runtime

The types are defined (`McpManager` trait, transport configs, server lifecycle) but there's no implementation. This is the single largest gap. Building a concrete `McpManager` with stdio/HTTP transport, tool discovery, and bridging into `ToolRegistry` would unlock the entire MCP ecosystem.

**Status:** v1 MVP DONE (2026-04-18). stdio transport + tools-only scope shipped end-to-end: hand-rolled JSON-RPC 2.0, `StdioTransport` with tolerant NDJSON buffering, `McpClient` request/response correlation, per-server `ServerSupervisor` with capped exponential backoff (500ms/2s/8s, cap 3 attempts), `StdMcpManager` façade, `McpToolAdapter` bridging into `ToolRegistry` with `mcp__<server>__<tool>` namespacing, `AppConfig.mcp` surface with `trust_level` + validation, server bootstrap wires `StdMcpManager` into `ServerRuntimeDependencies` and pre-seeds approval cache for trusted servers, observability via structured `tracing` events, `readOnlyHint`/`destructiveHint` annotations honored.

Totals: 405 unit + integration tests (+86 from baseline), all green.

### Deferred follow-ups (post-MVP)
- HTTP / Streamable-HTTP transport — types remain; implementation gated behind `streamable-http` cargo feature.
- OAuth 2.1 / PKCE / device flow.
- MCP resources + prompts (`read_resource` currently returns `McpResourceReadFailed { message: "not implemented" }`).
- Sampling, elicitation, roots notifications.
- `mcp/list`, `mcp/refresh`, `mcp/reload` server RPCs (Phase 9 of the plan — deferred; `runtime.rs` split is now done, so this is unblocked).
- TUI `/mcp` slash command.
- Hot reload of MCP tools into an in-flight session.

## 2. LLM-based Context Compaction

Replaces naive oldest-message drop with LLM-driven summarization per `docs/spec-context-management.md`. Raw history is never mutated; a single summary message replaces the compacted prefix in the prompt view, and a `CompactionSnapshot` makes the event recoverable.

**Status:** All 5 phases DONE. Wired through core + server with spec-compliant fallback to the legacy naive drop on summarizer failure.
- **Phase 1 — DONE:** Eligibility selector + summarization prompt (`crates/core/src/compaction/{mod,selector,prompt}.rs`; 15 tests)
- **Phase 2 — DONE:** `LlmContextCompactor` (`crates/core/src/compaction/llm_compactor.rs`; 9 tests — provider call, JSON parsing incl. fenced / nested-brace cases, error mapping)
- **Phase 3 — DONE:** `SnapshotStore` writes canonical JSON snapshots atomically under `<data_root>/snapshots/<session_id>/<turn_id>.json` (`crates/core/src/compaction/snapshots.rs`; 4 tests). Server-side `RolloutLine::CompactionSnapshot` emission done in Phase 5.
- **Phase 4 — DONE:** `ActiveCompaction` + `rebuild_prompt_view` pure function that swaps the compacted prefix for a single summary-bearing user message (`crates/core/src/compaction/prompt_view.rs`; 5 tests).
- **Phase 5 — DONE:** Full integration.
  - `crates/core/src/compaction/runner.rs` — `run_llm_compaction` stitches selector → compactor → session update with a per-session `AtomicBool` concurrency lock (5 tests).
  - `crates/core/src/session.rs` — `SessionState` carries `active_compaction: Option<ActiveCompaction>` and the `compacting` flag; new `to_prompt_messages()` applies the summary via `rebuild_prompt_view`; 3 tests.
  - `crates/core/src/query.rs` — `query()` now accepts `Arc<dyn ModelProviderSDK>`. Both compaction call sites (proactive budget + reactive `ContextTooLong`) try `run_llm_compaction` first and fall back to `compact_session` on `CompactionError` per spec §Error Handling. New `QueryEvent::ContextCompacted(CompactionOutcome)` is emitted for server-side persistence.
  - `crates/server/src/persistence.rs` — `RolloutStore::append_compaction_snapshot` writes JSON snapshot + `RolloutLine::CompactionSnapshot` rollout line atomically.
  - `crates/server/src/runtime.rs` — event handler consumes `QueryEvent::ContextCompacted`, emits a `TurnItem::ContextCompaction` to the journal, and persists the full snapshot record.
  - CLI and server call sites updated to pass `Arc::clone(&provider)`.
  - Totals: 333 unit + 27 integration = 360 tests, all green (+41 from baseline). No new warnings.

### Open follow-ups (non-blocking)
- `CompactionSnapshot.replaced_from_item_id` / `replaced_to_item_id` / `prompt_segment_order` are currently `ItemId::new()` placeholders. The in-memory `SessionState` tracks a flat `Vec<Message>` rather than item ids, so no authoritative mapping exists yet. A future change can thread real item ids once sessions track them (this is orthogonal to the compaction correctness — the JSON snapshot is recoverable today via the `summary_item_id` + rollout journal).
- `SummaryModelSelection::UseAxiliaryModel` currently falls back to the turn model because auxiliary-model resolution isn't wired yet. Hook into the model catalog when the auxiliary model lands.
- `micro_compact` (`crates/core/src/query.rs`) is still in place; spec says model truncation policy should replace it. Remove once `ModelPreset.truncation_policy` is honored end-to-end.

## 3. Approval Flow

End-to-end approval workflow: tool needs permission → orchestrator suspends → server broadcasts approval request → TUI prompts user → user approves/denies → decision flows back to the orchestrator.

**Status:** All 4 phases DONE.
- **Phase 1 — DONE:** `ApprovalManager` with register/respond/cancel; `ApprovalChannel` trait on `ToolContext`; orchestrator `Ask` branch uses channel; `approval/respond` handler wired.
- **Phase 2 — DONE:** `QueryEvent::ApprovalRequest` + `ServerEvent::ApprovalRequested`; `ServerApprovalChannel` emits events via `mpsc`; `ApprovalRequestItem` / `ApprovalDecisionItem` emitted as turn items (already persisted to rollout via `emit_turn_item` → `append_item`).
- **Phase 3 — DONE (2026-04-18):**
  - `crates/protocol/src/approval.rs` — added `ApprovalRespondResult`.
  - `crates/client/src/stdio.rs` — `StdioServerClient::approval_respond()` method.
  - `crates/tui/src/events.rs` — `WorkerEvent::ApprovalRequest`/`ApprovalResolved` variants (carrying session_id + turn_id so responses know where to route); `TranscriptItemKind::ApprovalPrompt`/`ApprovalResolution`.
  - `crates/tui/src/worker.rs` — `OperationCommand::RespondApproval`, `QueryWorkerHandle::respond_approval()`, server `approval/requested` event routed into the worker.
  - `crates/tui/src/{app,runtime,worker_events}.rs` — `PendingApprovalState` on `TuiApp`, approval prompts pushed to transcript, y/n/Esc intercepted in `handle_key` when the composer is blank, `submit_pending_approval()` helper updates the transcript optimistically and calls into the worker.
  - `crates/tui/src/render/theme.rs` + `render/transcript.rs` + `transcript.rs` — approval-kind styling + rendering.
- **Phase 4 — DONE (2026-04-18):**
  - `crates/safety/src/lib.rs` — `StaticPermissionPolicy::decide` consults `snapshot.approval_cache` (tool_scopes → blanket tool allow; path_scopes → prefix match for `FileWrite`; host_scopes → exact match for `Network`) before returning `Ask`. 4 new cache-aware tests.
  - `crates/server/src/execution.rs` — `RuntimeSession.approval_cache: ApprovalCache`; initialized to default in all three construction sites (fresh session, resumed session, fork).
  - `crates/server/src/persistence.rs` + `runtime.rs` — approval item rollout persistence confirmed already wired via `emit_turn_item` (Phase 2 left it in place).

### Phase 4b — DONE (same session, 2026-04-18)
- `RuntimeSession.approval_cache` is now `Arc<tokio::sync::Mutex<ApprovalCache>>`, shared with the tool orchestrator via `ToolOrchestrator::with_approval_cache()`.
- `ApprovalChannel::request_approval()` threads `tool_name`; `PendingApproval` and `ResolvedApproval` carry it back.
- `handle_approval_respond` populates `approval_cache.tool_scopes` on `Approve` + `Session`/`Tool` scope.
- The orchestrator short-circuits to `invoke_tool` when the shared cache already contains the tool name, bypassing the legacy policy and approval channel entirely. Test `cached_tool_scope_skips_ask` proves it.

### Future polish (not blocking anything)
- Full migration of the orchestrator from the legacy `PermissionPolicy` trait to `lpa_safety::PermissionPolicy` would give per-resource scoping (`PathPrefix`, `Host`). Requires tool-level resource declarations so the orchestrator knows what to request. Not required for the current cache-hookup to work — that handles the common "approve this tool for the session" case.

## 4. OS-Level Sandboxing

The spec defines `SandboxMode::Restricted/External` but nothing is implemented. Platform-specific sandboxing (seccomp/landlock on Linux, seatbelt/sandbox-exec on macOS, restricted tokens on Windows) would make the agent safe to run untrusted code.

**Status:** Not started. Types exist in `crates/safety/src/lib.rs`: `SandboxMode` enum (`Unrestricted`/`Restricted`/`External`), `SandboxPolicyRecord`, `NetworkPolicy`. Nothing enforces them at process-spawn time.

## 5. Web/Desktop Client

The server already speaks JSON-RPC over WebSocket. A web frontend or Electron app just needs to connect and render events. The protocol crate defines all the types.

**Status:** Not started

## 6. Subagent/Task Orchestration

`TaskTool` is a stub that returns an acknowledgment. Implementing actual subagent dispatch (spawn a child agent with scoped context, collect results) would enable complex multi-step workflows.

**Status:** Not started. The `lpa-tasks` crate has scaffolding (`Task` trait, `TaskManager` with lifecycle tracking, `TaskNotification` — fully tested but not wired). `TaskTool` in `crates/tools/src/task.rs` is intentionally not registered in `register_builtin_tools()`. Key blocker: `ToolContext` lacks access to `Arc<dyn ModelProviderSDK>` and `Arc<ToolRegistry>`, which a nested `query()` call would need.

## 7. LSP Integration

`LspTool` is a stub. Connecting to actual language servers (via `tower-lsp` or custom transport) would give the agent real code intelligence (go-to-def, diagnostics, completions).

**Status:** Not started

## 8. Tool Progress Reporting

`ToolProgressEvent` types exist but aren't wired. Adding real-time progress events (especially for long-running bash/patch operations) would improve UX significantly.

**Status:** Not started. `ToolProgressEvent` enum (`Status`/`ByteProgress`/`SubCommand`) defined in `crates/tools/src/tool.rs`. `ToolProgressReporter` trait + `NullToolProgressReporter` in `crates/tools/src/runtime/types.rs`. `RuntimeToolExecutor` always uses the null reporter. No `QueryEvent` variant exists for progress. Protocol layer has `ItemDeltaKind::CommandExecutionOutputDelta` ready as a wire carrier.

## 9. Git Ghost Snapshots

The `CompactionSnapshot` type supports `JsonAndGit` backend. Implementing automatic git branching on compaction would enable session rollback ("undo everything the agent did in the last 5 turns").

**Status:** Not started

## 10. Skill Watcher

The config has `watch_for_changes` but no inotify/FSEvents implementation. Hot-reloading skills as you edit them would be useful.

**Status:** Not started

## 11. Session Sharing/Collaboration

The server supports multiple connections but concurrent access isn't robust. Adding proper multi-client session coordination would enable pair-programming with the agent.

**Status:** Not started

---

## Completed Work

### Rebrand (Complete)
Full rebrand from ClawCR to Launchpad Agent across:
- Cargo.toml files (workspace + per-crate), Rust source files
- Env vars (`CLAWCR_*` → `LPA_*`), config dirs (`~/.clawcr` → `~/.launchpad/agent`), project config dirs (`.clawcr/` → `.lpagent/`)
- README, AGENTS.md
- Follow-up sweep (2026-04-18): `crates/client/Cargo.toml` description, agent identity in `crates/core/default_base_instructions.txt` + 8 per-model overrides in `crates/core/models.json` (`"You are Clawcr"` → `"You are Launchpad Agent"`), `docs/design-overview.md` + 5 `spec-*.md` titles (`ClawCodeRust` → `Launchpad Agent`), and 3 `CLAWCR_HOME` references in `docs/spec-app-config.md` → `LPA_HOME`.

### Test Suite Fix (Complete)
Fixed 4 pre-existing `skills_integration` test failures caused by macOS `/var` → `/private/var` canonicalization. All 142 tests now pass.

### Approval Flow Phase 1 (Complete)
- `crates/server/src/approval.rs` — `ApprovalManager`, `PendingApproval`, `ApprovalResult`, `ResolvedApproval`, `SharedApprovalManager`
- `crates/tools/src/context.rs` — `ApprovalChannel` trait + `approval_channel` field on `ToolContext`
- `crates/tools/src/orchestrator.rs` — `Ask` branch uses approval channel, falls back to denial
- `crates/server/src/runtime.rs` — `approval/respond` handler, cancellation on turn interrupt
- All `ToolContext` construction sites updated with `approval_channel: None`

### Approval Flow Phase 2 (Complete)
- `crates/core/src/query.rs` — `QueryEvent::ApprovalRequest` variant
- `crates/protocol/src/event.rs` — `ServerEvent::ApprovalRequested` broadcast variant
- `crates/server/src/approval_channel.rs` — `ServerApprovalChannel` emits `QueryEvent::ApprovalRequest` via `mpsc`
- `crates/server/src/runtime.rs` — Orchestrator wired with approval channel in `execute_turn`; `ApprovalRequestItem` emitted as turn item; `ApprovalDecisionItem` emitted on approval response; `ServerRequestResolved` broadcast

### Google Gemini Provider (Complete)
Full Google Gemini integration across all 4 phases. Users can configure via `GEMINI_API_KEY` or `GOOGLE_API_KEY` env vars and select any Gemini model.

**Phase 1 — Protocol + core types:**
- `crates/protocol/src/model.rs` — `GoogleApi` enum, `ProviderFamily::Google` variant, `google()` constructor, `as_str()`, `deserialize_provider()` handles `"google"`/`"gemini"`
- `crates/core/src/config/provider.rs` — `ProviderWireApi::GoogleGenerateContent` variant, `provider_family()`, `default_for_provider()`, `Deserialize` impl with `"google"`/`"gemini"` aliases
- `crates/provider/src/provider.rs` — `ProviderCapabilities::google()` constructor

**Phase 2 — Provider implementation:**
- `crates/provider/src/google/mod.rs` — Re-exports
- `crates/provider/src/google/generate_content.rs` — `GoogleProvider` struct, `ModelProviderSDK` + `ProviderAdapter` impls, request builder (contents, systemInstruction, generationConfig, functionDeclarations, thinkingConfig), response parser (candidates, functionCall, thought parts, usageMetadata), SSE streaming via `EventSource`, 3 unit tests
- `crates/provider/src/google/role.rs` — `GoogleRole` enum (`User`, `Model`) with Display + FromStr
- `crates/provider/src/lib.rs` — `pub mod google`

**Phase 3 — Server + TUI wiring:**
- `crates/server/src/provider_config.rs` — Provider instantiation, `GOOGLE_API_KEY`/`GEMINI_API_KEY` env var fallbacks, `default_model_for_provider()` → `"gemini-2.5-flash"`
- `crates/tui/src/worker.rs` — `build_validation_provider()` Google arm
- `crates/cli/src/agent.rs` — `ProviderFamily::Google { .. }` arms
- `crates/tui/src/onboarding.rs` — Wire API serialization, matching, parsing arms

**Phase 4 — Model catalog:**
- `crates/core/models.json` — Gemini 2.5 Pro (thinking toggle, 1M ctx, 64K output), Gemini 2.5 Flash (thinking toggle, 1M ctx, 64K output), Gemini 2.0 Flash (non-thinking, 1M ctx, 8K output)

### Codebase Hardening & Architecture Overhaul (Complete — 2026-04-21)

**419 tests pass, zero clippy warnings.**

#### Typed Provider Errors
Replaced the entire string-matching error classification system (~70 lines of `classify_error()` parsing `"429"`, `"context_too_long"`, etc.) with a structured `ProviderError` enum (11 variants) in `crates/provider/src/error.rs`. The `ModelProviderSDK` trait now returns `Result<_, ProviderError>` instead of `anyhow::Result`. All three providers (Anthropic, OpenAI, Google) map HTTP status codes to typed variants. `AgentError::Provider` wraps `ProviderError` instead of `anyhow::Error`. The `ErrorClass` enum and `classify_error()` function deleted from `query.rs`.

#### Strict Tool Input Validation
8 tools that silently defaulted required parameters to empty values (`unwrap_or("")` / `unwrap_or_default()`) now return errors: `websearch`, `todo`, `question`, `skill`, `apply_patch`, `webfetch`, `lsp`, `task`.

#### Dead Code Removal
Deleted `crates/tools/src/spec.rs` (540 lines) — an earlier draft of the runtime tool system completely superseded by `runtime/types.rs` + `runtime/registry.rs`.

#### Memory Leak Fixes
Replaced `Box::leak` in `BashTool::description()` and `WebSearchTool::description()` with `OnceLock<String>` — computed once, cached, no leak.

#### Shell Detection
`platform_shell()` in `shell_exec.rs` now reads `$SHELL` on Unix to detect zsh/fish instead of hardcoding bash.

#### Test Cleanup
Replaced 10 `panic!` calls in provider test code with proper assertion patterns (`assert!(matches!(...))` and `if let ... else { panic! }`).

#### File Splitting (5 files, ~5800 lines total split across 28 new modules)
- `apply_patch.rs` (1490 → ~170): `apply_patch/{mod,types,parse,apply,hunk_match,tests}.rs`
- `server/src/runtime.rs` (2187 → ~170): `runtime/{handlers_session,handlers_turn,handlers_events,execute_turn,session_titles,items,connection_runtime}.rs`
- `tui/src/worker.rs` (1326 → ~530): `worker/{mod,event_mapping,tool_render,history,provider_validate,tests}.rs`
- `tui/src/selection.rs` (1249 → ~140): `selection/{mod,model,slash_commands,onboarding,panel_accept,rollout_files}.rs`
- `core/src/query.rs` (1147 → ~480): `query/{mod,compaction,prefetch,connection_test,tests}.rs`

#### Hierarchical AGENTS.md Loading
`load_prompt_md()` in `crates/core/src/query/prefetch.rs` now walks from `cwd` upward to the project root (detected via `.git` or configured `project_root_markers`), collecting all `AGENTS.md`/`CLAUDE.md` files. Loads in order: root → ... → cwd (most specific last). Deduplicates by canonical path. Falls back to single-directory loading when no project root is found.

---

## Reference: Where will we land vs. Crush / Claude Code?

Honest projection after everything above ships: **closer to Crush than to Claude Code.** Good enough to daily-drive if you like it, but Claude Code users will notice specific gaps.

### What items 1-11 give us
Multi-provider streaming (Anthropic / OpenAI / Google) + core tool use (bash / read / write / edit / glob / grep / apply_patch / webfetch) + context compaction with snapshots + sessions with resume + approval flow with cache + TUI + MCP + subagents + tool progress + LSP + ghost snapshots + sandboxing. That's Crush-tier or slightly ahead (ghost snapshots and OS sandboxing are unusual).

### Claude Code features NOT on this wishlist
These are the likely "I miss this" items after a week of dogfooding:

| Feature | Current state in launchpad-agent | Why it matters |
|---|---|---|
| **Hooks system** (PreToolUse / PostToolUse / SessionStart / Stop / UserPromptSubmit / etc.) | Not present | Power users live in hooks — deterministic guardrails, custom automation, shell integrations. Big gap. |
| **Rich slash command set** (`/agents`, `/cost`, `/mcp`, `/model`, `/memory`, `/compact`, `/hooks`, `/doctor`) | Partial — basic commands only | Discoverability + day-to-day ergonomics. |
| **Skills / plugin ecosystem** | Partial — skills are discovered from disk (`crates/core/src/skills.rs`), no install / marketplace story | Claude Code's skills are a significant multiplier for real workflows. |
| **`CLAUDE.md` / `AGENTS.md` hierarchical auto-load** | DONE — walks cwd upward to project root (`.git`), loads all instruction files | Was a monorepo gap, now handled. |
| **Image paste into chat** | Not present | Design reviews, screenshot debugging, vision-model workflows. |
| **`WebSearch` tool** | Only `WebFetch` | Agent can't answer "what's the current state of X?" without a search step. |
| **Output styles / personality modes** | Not present | Low priority but a visible polish gap. |
| **Customizable status line** | Not present | Cosmetic. |
| **Background bash processes** (`run_in_background`, monitor output, kill) | Single-shot only | Needed for `npm run dev` while the agent keeps working. |
| **Notebook (`.ipynb`) editing** | Not present | Niche but real for data science users. |

### The recommendation when items 1-11 are done
Dogfood for a week. Note every time you reach for Claude Code instead. The top one or two misses from the table above become the next set of features — don't speculatively add them all before you know which ones bite.
