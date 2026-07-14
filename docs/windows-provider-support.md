# Windows Provider Source Matrix

This matrix defines the source discovery work for the Windows edition. It records paths and mechanisms,
never credential values. Source order remains provisional until each provider parent validates the
source against a real native Windows installation and the provider's current behavior.

The P1 probe ran on Windows 11 x64 build `10.0.26200`. It checked only path existence, environment
variable names, and process names. It did not read credential files, databases, environment values,
process command lines, or Windows Credential Manager entries. P3 then exercised the three established
providers through the trusted backend; its native probe reports only provider ID, outcome category,
metric count, and history availability.

## Source Matrix

| Provider | Ordered native Windows sources | P1 host evidence | Remaining validation |
| --- | --- | --- | --- |
| Claude | `CLAUDE_CONFIG_DIR/.credentials.json` when set; otherwise `%USERPROFILE%/.claude/.credentials.json`. History comes from the selected root's `projects` tree | Default credentials and history were exercised through bounded, incremental readers | Windows Credential Manager and Claude Desktop sources remain disabled because their target and format are unverified |
| Codex | `CODEX_HOME/auth.json` when set; otherwise `%USERPROFILE%/.config/codex/auth.json`, then `%USERPROFILE%/.codex/auth.json`. History comes from every selected home's `sessions` and `archived_sessions` | Override/default auth and session roots were exercised without reporting paths or values | API-key-only authentication cannot provide subscription usage; unverified Credential Manager sources remain disabled |
| Cursor | `%APPDATA%/Cursor/User/globalStorage/state.vscdb`; authenticated account export for history | Locked WAL-mode SQLite, session-only token refresh, and account-wide export were exercised without modifying the source database | Unverified Windows Credential Manager targets remain disabled |
| Antigravity | Native ToolHelp process discovery for `language_server.exe` and `agy.exe` | No matching process was present; fixture quota mapping is verified | Explicit limitation: no verified safe Windows source exposes the authenticated loopback port and CSRF token; Credential Manager and command-line scraping remain disabled |
| Copilot | `%APPDATA%/github-copilot/apps.json`, then `hosts.json`, then `%APPDATA%/GitHub CLI/hosts.yml` | Ordered parsers and GitHub-host scoping are verified with native files | GitHub CLI Credential Manager fallback remains disabled until its exact Windows target is verified |
| Devin | `%USERPROFILE%/.local/share/devin/credentials.toml`; `%APPDATA%/Devin/User/globalStorage/state.vscdb`; `%APPDATA%/Devin - Next/User/globalStorage/state.vscdb` | TOML precedence, HTTPS server validation, and locked read-only SQLite are verified | No remaining native source gap |
| Grok | `GROK_HOME/auth.json` when set; otherwise `%USERPROFILE%/.grok/auth.json`; history at the selected root's `logs/unified.jsonl` | Candidate accounts, generation-checked atomic refresh, billing fixtures, and bounded incremental history are verified | No native credential was present on the probe host for a live API check |
| OpenCode | `OPENCODE_DATA_DIR`; `XDG_DATA_HOME/opencode`; otherwise `%USERPROFILE%/.local/share/opencode`; every bounded `opencode*.db` file | Go auth, locked SQLite, hosted spend, and UTC Go windows are verified | `%LOCALAPPDATA%/opencode` is not guessed because upstream precedence does not establish it |
| OpenRouter | DPAPI-protected OpenUsage key; compatibility files `%USERPROFILE%/.config/openusage/openrouter.json` then `%USERPROFILE%/.config/openrouter/key.json`; environment `OPENROUTER_API_KEY` then `OPENROUTER_KEY` | Protected save, replace, status, delete, override, and fixture API mapping are verified | No key was present on the probe host for a live API check |
| Z.ai | DPAPI-protected OpenUsage key; compatibility files `%USERPROFILE%/.config/openusage/zai.json` then `%USERPROFILE%/.config/zai/key.json`; environment `ZAI_API_KEY` then `GLM_API_KEY` | Protected lifecycle and quota/subscription fixture mapping are verified | No key was present on the probe host for a live API check |

## Source Rules

- Native Windows sources are never inferred from macOS paths by string replacement.
- Environment probes return presence and a secret value only inside the trusted Rust provider path;
  diagnostics expose presence only.
- A missing source means not logged in. An unreadable, locked, malformed, or access-denied source is a
  distinct provider error.
- OpenUsage never writes to another application's database or credential store unless the provider's
  refresh contract explicitly requires updating the same verified credential source.
- Third-party Windows Credential Manager access stays disabled until the exact target, persistence
  behavior, and account scoping are reproduced without logging secrets.
- Claude and Codex refresh only their verified source file, with generation checks and atomic
  replacement. Cursor refresh tokens stay in memory and OpenUsage never writes Cursor's database.
- WSL distributions are outside this matrix. No fallback crosses into WSL automatically.
- Real-provider verification records only source kind, outcome category, and normalized response shape.
  Tokens, cookies, usernames, tenant identifiers, raw rows, and full paths are not captured.
- App-owned API keys take precedence over compatibility files and environment variables. Compatibility
  files remain read-only; new keys are DPAPI protected and cannot be read back through frontend IPC.
