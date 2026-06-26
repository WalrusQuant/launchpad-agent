# Parity Roadmap — everything to bring `lpagent` to Claude Code CLI level

The complete feature surface of a top-tier CLI coding agent (measured against Claude Code,
cross-checked with Crush / opencode). Status verified against the `lpagent` codebase.

Legend: `✅` done · `◑` partial · `✗` missing. Every `◑`/`✗` has concrete tasks.

---

## 1. CLI invocation & headless

- ✅ Interactive TUI; ✅ `onboard`, `prompt` (one-shot), `doctor`, `server` subcommands
- ✅ **`-p` / `--print` non-interactive mode** (run, emit result, exit; no TUI)
- ✗ **`--output-format text|json|stream-json`** (structured machine output)
- ✗ **`--input-format text|stream-json`**
- ✅ **stdin piping** (`cat x | lpagent -p`)
- ✅ **`--continue` (resume most recent in cwd)** and **`--resume <id>`** flags
- ✅ **`--model` flag**; ✗ **`--fallback-model`**
- ✗ **`--permission-mode` flag**
- ✅ **`--allowed-tools` / `--disallowed-tools` flags**
- ✅ **`--append-system-prompt` / `--system-prompt` flags**
- ✗ **`--add-dir` (extra working roots)**
- ✗ **`--mcp-config` / `--settings` flags**
- ✅ **`--session-id` flag** (resume-or-create under a caller-chosen id)
- ✅ **`--verbose` / `--debug` flags**
- ✅ **`--dangerously-skip-permissions`**
- ✅ **Documented exit codes** for scripting (0 success / 1 failure / 2 usage)
- ✗ **SDK / library API** (embeddable client contract for the Launchpad terminal)

## 2. Slash commands

- ✅ `/config` `/configure` `/exit` `/model` `/new` `/reasoning` `/rename` `/sessions` `/skills` `/status` `/thinking`
- ✅ **`/help`**
- ✅ **`/clear`** (reset context, keep session)
- ✅ **`/compact`** (manual; focus arg not yet supported)
- ✗ **`/cost`** (usage + $)
- ✗ **`/context`** (context-window usage visualization)
- ✗ **`/init`** (scan repo → generate `AGENTS.md`)
- ✗ **`/memory`** (open/edit memory files)
- ✗ **`/review`** (review working diff)
- ✗ **`/agents`** (manage subagent definitions)
- ✗ **`/hooks`** (view/manage hooks)
- ✗ **`/mcp`** (inspect/manage MCP servers, auth)
- ✗ **`/permissions`** (view/edit permission rules + mode)
- ✗ **`/login` / `/logout`** (provider auth)
- ✅ **`/export`** (transcript export to Markdown)
- ✗ **`/rewind`** (checkpoint restore)
- ✗ **`/vim`** (toggle vim mode)
- ✗ **`/theme`** (switch theme)
- ✗ **`/output-style`** (switch persona/output style)
- ✗ **`/statusline`** (configure custom status line)
- ✗ **`/add-dir`** (add working root mid-session)
- ✅ **`/bug` / `/feedback`** (report link)
- ✗ **`/plugin`** (manage plugins)
- ✗ **`/install-github-app`** (GitHub Action setup)
- ◑ **`/release-notes`** (version + link) ✅ · ✗ **`/upgrade`** (self-update)
- ✗ **User-defined custom slash commands** — see §7

## 3. Configuration & settings

- ✅ User config (`~/.launchpad/agent/config.toml`) + project (`<ws>/.lpagent/config.toml`)
- ✗ **Local (git-ignored) settings layer** + **enterprise/managed settings**
- ✗ **`[permissions]` allow / deny / ask rule arrays** (tool- and pattern-scoped, e.g. `Bash(npm run test:*)`, `Read(./secrets/**)`, `WebFetch(domain:...)`)
- ✗ **`additionalDirectories`** config
- ✗ **`apiKeyHelper`** (script that emits a key/token)
- ✗ **env-var interpolation in config**
- ✗ **`statusLine` config**, **`outputStyle` config**, **`includeCoAuthoredBy`**, **`cleanupPeriodDays`**
- ✗ **proxy / custom CA config surface**

## 4. Memory / context loading

- ◑ **Project memory** — prefetch exists; missing: load `AGENTS.md`/`CLAUDE.md` up the dir tree + user + enterprise scope, defined precedence
- ✗ **`@path` imports inside memory files**
- ✅ **`#`-prefixed input appends a memory line** (to `AGENTS.md`/`CLAUDE.md`)
- ✗ **`/memory` editing**, ✗ **`/init` generation**
- ✗ **`@file` / `@dir` / `@symbol` mentions in the prompt** (+ autocomplete)
- ✅ Auto-compaction (LLM + naive fallback); ✅ **manual `/compact`**; ✅ **`/clear`**
- ✗ **`/context` usage visualization**

## 5. Tools (built-in)

- ✅ Bash (timeout), Read (text), Write, Edit (`apply_patch`), Glob, Grep, WebFetch, WebSearch, TodoWrite, Plan (`update_plan`), Question
- ✗ **Bash background execution** + **`BashOutput`** (poll) + **`KillShell`**
- ✗ **Read images** (→ image content block), **PDF** (pages), **Jupyter notebooks** (cells)
- ✗ **`NotebookEdit` tool**
- ✅ **`ls` / list-directory tool**
- ✗ **`ExitPlanMode` tool** (plan-mode approval) — see §11
- ✗ **`Task` tool** (subagent dispatch) — see §6
- ✗ **Git/PR tooling** (structured status/diff/commit; PR create/view) — currently raw Bash only
- ✅ Diff display + syntax highlighting

## 6. Subagents / task delegation

- ✗ **`Task` tool** (restore deleted scaffolding as a real impl; register it)
- ✗ **Agent definitions** (`.lpagent/agents/*.md`, frontmatter `name`/`description`/`tools`/`model`/system prompt; user + project scope)
- ✗ **Agent registry + resolver**
- ✗ **Isolated sub-session dispatch** (own context window + tool subset; return result to parent)
- ✗ **Parallel subagents + result aggregation**
- ✗ **Recursion/depth guard** (use the `tasks` crate seam)
- ✗ **Proactive auto-delegation** (model picks an agent by description)
- ✗ **`/agents` management UI**

## 7. Custom slash commands

- ✗ **Discovery** (`<ws>/.lpagent/commands/`, `~/.launchpad/agent/commands/`)
- ✗ **Markdown format + frontmatter** (`description`, `argument-hint`, `allowed-tools`, `model`)
- ✗ **Arg substitution** (`$ARGUMENTS`, `$1`…), **`@file` embedding**, **`` !`bash` `` execution**
- ✗ **Namespacing** (`/dir:command`)
- ✗ **Surfaced in slash menu + autocomplete**

## 8. Hooks

- ✗ **Hook config schema** (event → matcher → command)
- ✗ **Events**: `PreToolUse`, `PostToolUse`, `UserPromptSubmit`, `SessionStart`, `SessionEnd`, `Stop`, `SubagentStop`, `PreCompact`, `Notification`
- ✗ **Matchers** (tool-name globs / source filters)
- ✗ **Runner** (exec shell, event JSON on stdin, capture stdout/exit, timeout)
- ✗ **Decision protocol** (allow / deny / ask / inject-context / modify-input)
- ✗ **Orchestrator wiring** (PreToolUse gate, PostToolUse, UserPromptSubmit, SessionStart inject)
- ✗ **`/hooks` viewer**

## 9. Plugins

- ✗ **Manifest format** (name/version/provides: commands/agents/hooks/mcp)
- ✗ **Loader** (user + project plugin dirs)
- ✗ **Install** from local path / git / marketplace index
- ✗ **Bundle + register** a plugin's commands/agents/hooks/MCP on load
- ✗ **Enable / disable / uninstall + `/plugin`**
- Depends on §6, §7, §8, §10

## 10. MCP

- ✅ stdio transport + tool discovery + `trust_level` approval pre-seed
- ✗ **Streamable-HTTP transport** (remove feature gate)
- ✗ **SSE transport**
- ✗ **OAuth** for remote servers (auth-code + token store + refresh)
- ✗ **Resources** (list/read + `@server:resource` mentions)
- ✗ **Prompts** (expose as slash commands)
- ✗ **Reconnect/backoff** for HTTP/SSE
- ✗ **Server→client: sampling, elicitation, roots**
- ✗ **`/mcp` management UI** (status, auth, enable/disable)
- ✗ **Project-scoped `.mcp.json`-style declaration + per-project enable**

## 11. Agent control / workflow

- ✗ **Plan mode** (read-only pass) + **`ExitPlanMode`** approval transition
- ◑ **Permission modes** — have Allow/Deny/Ask + AutoApprove + workspace sandbox; missing **named `default` / `acceptEdits` / `plan` / `bypassPermissions` modes** + per-session toggle (key + `/permissions`)
- ✗ **Checkpointing / rewind** (snapshot conversation + working-tree per turn; `/rewind` to restore messages and/or files)
- ◑ **Steering / queued input** — loop drains `pending_user_prompts`; finish: accept input mid-turn, inject next iteration, composer affordance
- ✗ **Background tasks** (run command/agent in background, poll, kill; status-line surfacing)
- ✗ **Scheduled / recurring runs** (cron-like jobs → headless run + notify)
- ✅ Thinking/reasoning control · ✅ TodoWrite · ✅ Plan tool

## 12. Providers / models / auth

- ✅ 25 presets; 3 native wire APIs (Anthropic/OpenAI/Google); model picker; key reuse
- ✗ **Prompt caching** (Anthropic `cache_control`, OpenAI/Gemini caching) — only usage tokens tracked, not wired into request build
- ✗ **`tool_choice` / forced-tool passthrough** (types described in comments, not built)
- ✗ **Structured outputs / JSON mode** (`response_format` / `json_schema`)
- ✗ **Model fallback chain** (retry on alternate model on failure/rate-limit)
- ✗ **OAuth / subscription login** (`/login`, `/logout`) — API keys only
- ✗ **Cloud gateway auth**: AWS Bedrock (SigV4), Google Vertex, Azure OpenAI
- ✗ **`apiKeyHelper`** support

## 13. Permissions & sandboxing

- ◑ Rule-based policy + approval cache + workspace-write sandbox (deny writes/shell outside cwd)
- ✗ **OS-level sandboxing / process isolation** (landlock/seccomp/bubblewrap/seatbelt) — `SandboxMode::External` is stubbed-out (fails closed)
- ✗ **Network egress control** (allow/deny hosts at process level)
- ✗ **Config-driven granular rules** (per-command Bash allow/deny lists, path rules, WebFetch domain rules)
- ✗ **Named permission modes** (see §11)

## 14. Input / editor UX

- ◑ **Image input** — types/plumbing exist; finish paste/drag/attach-by-path → image content block; verify across all 3 providers
- ◑ **Autocomplete** — slash ✅; missing **file-path** + **`@`-mention** + **command-arg** completion
- ✗ **Vim mode** (modal composer)
- ◑ **Themes** — one built-in; missing **multiple themes + `/theme` switch + config**
- ✗ **Custom status line** (user script/format)
- ✗ **Notifications** (terminal bell / desktop on done / awaiting input)
- ✗ **`/terminal-setup`** (shift+enter / keybinding helper)
- ✅ status line / token counter · ✅ collapsible reasoning · ✅ welcome banner

## 15. Output / display

- ✅ Diff + syntax highlighting; markdown rendering
- ✗ **Per-hunk edit accept/reject** + **undo last edit**
- ✗ **Output styles / personas** (default / explanatory / learning / custom)

## 16. Sessions / history

- ✅ persistence · ✅ resume · ✅ fork
- ◑ **`/export`** (Markdown transcript) ✅ · ✗ **JSON export / share link**
- ✗ **Conversation search** (across sessions/messages)
- ✅ **CLI `--continue` / `--resume` / `--session-id`** (headless; every `-p` run persists a rollout, see §1)

## 17. Cost / usage / telemetry

- ✅ token counting (in/out)
- ✗ **Cost in $** (per-model pricing table; `/cost`; per-session + cumulative)
- ✗ **Spend / budget limits** (cap tokens/$ per turn or session; stop when exceeded)
- ✗ **Telemetry / OpenTelemetry export** (usage/latency/errors, opt-in)

## 18. Git / GitHub

- ✗ **Structured git helpers** (status/diff/commit)
- ✗ **PR create / view** (gh CLI or API)
- ✗ **`/review`** over working diff
- ✗ **`/pr-comments`** (read PR review threads)
- ✗ **`/install-github-app`** (CI/Action integration)
- ✗ **`includeCoAuthoredBy` commit trailer config**

## 19. Ops / lifecycle

- ✅ `/doctor` · ✅ `/help` · ✅ `/bug` / `/feedback` · ✅ `/release-notes`
- ✗ **Self-update / version check / `/upgrade`**
- ✗ **`/privacy-settings`**
- ◑ **Skills** — have skill tool + catalog; missing **hot-reload** (`watch_for_changes` config exists, no fs-events impl)

---

*Scope note: IDE/editor plugins and desktop/web GUI clients are intentionally excluded — the
Launchpad terminal is the host UI; `lpagent` is the complementary agent backend, so its own TUI
is a reference client and the priority is engine + protocol + the features above.*
