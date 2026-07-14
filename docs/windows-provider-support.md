# Windows Provider Source Matrix

This matrix defines the source discovery work for the Windows edition. It records paths and mechanisms,
never credential values. Source order remains provisional until each provider parent validates the
source against a real native Windows installation and the provider's current behavior.

The P1 probe ran on Windows 11 x64 build `10.0.26200`. It checked only path existence, environment
variable names, and process names. It did not read credential files, databases, environment values,
process command lines, or Windows Credential Manager entries.

## Source Matrix

| Provider | Ordered native Windows sources | P1 host evidence | Remaining validation |
| --- | --- | --- | --- |
| Claude | `CLAUDE_CONFIG_DIR/.credentials.json`; `%USERPROFILE%/.claude/.credentials.json`; verified Windows Credential Manager target if Claude Code uses one; `CLAUDE_CODE_OAUTH_TOKEN` as inference-only metadata; native Claude Desktop storage only if its DPAPI/cookie format is safely supported | Default credentials file exists; no override or OAuth-token environment name is present | Credential Manager target naming and Claude Desktop access are unknown and must not be guessed in P3 |
| Codex | `CODEX_HOME/auth.json`; `%USERPROFILE%/.config/codex/auth.json`; `%USERPROFILE%/.codex/auth.json`; verified Windows Credential Manager target if present. History comes from the selected home's `sessions` and `archived_sessions` | `CODEX_HOME` is present by name; default auth and sessions paths exist | Confirm override path without logging it; verify whether native Codex writes Credential Manager data in P3 |
| Cursor | `%APPDATA%/Cursor/User/globalStorage/state.vscdb`; verified Windows Credential Manager targets for access and refresh tokens; authenticated account export for history | Cursor state database and running Cursor processes exist | Prove read-only WAL handling and exact Credential Manager target names in P3 |
| Antigravity | Native `language_server` or `agy` process discovery plus loopback quota service; `%APPDATA%/Antigravity IDE/User/globalStorage/state.vscdb` and legacy `%APPDATA%/Antigravity/User/globalStorage/state.vscdb` only for verified non-secret metadata; verified Windows Credential Manager target for the `gemini` / `antigravity` token | No matching database or process was present | Process command-line access, local TLS, and Credential Manager naming remain unknown for P4 |
| Copilot | `%APPDATA%/github-copilot/apps.json` then `hosts.json` when native clients create them; `%APPDATA%/GitHub CLI/hosts.yml`; verified GitHub CLI Windows Credential Manager target | None of the candidate files was present | Confirm GitHub CLI and editor storage order and Credential Manager naming in P4 |
| Devin | `%USERPROFILE%/.local/share/devin/credentials.toml`; `%APPDATA%/Devin/User/globalStorage/state.vscdb`; `%APPDATA%/Devin - Next/User/globalStorage/state.vscdb` | No candidate source was present | Validate whether the CLI uses the cross-platform home path and prove locked SQLite handling in P4 |
| Grok | `GROK_HOME/auth.json`; `%USERPROFILE%/.grok/auth.json`; logs at the selected home's `logs/unified.jsonl` | No override, auth file, log, or process was present | Validate native Windows CLI behavior and atomic refresh-token persistence in P4 |
| OpenCode | `OPENCODE_DATA_DIR`; `XDG_DATA_HOME/opencode`; `%USERPROFILE%/.local/share/opencode`; a native `%LOCALAPPDATA%/opencode` source only if upstream Windows behavior confirms it | No candidate directory or override was present | Upstream Windows data-root precedence and hosted SQLite locking remain unknown for P4 |
| OpenRouter | `OPENROUTER_API_KEY`, then `OPENROUTER_KEY`, then compatibility reads from `%USERPROFILE%/.config/openusage/openrouter.json` and `%USERPROFILE%/.config/openrouter/key.json`; newly entered keys use protected OpenUsage storage | No environment name or compatibility file was present | Validate protected save/status/delete flow in P4 |
| Z.ai | `ZAI_API_KEY`, then `GLM_API_KEY`, then compatibility reads from `%USERPROFILE%/.config/openusage/zai.json` and `%USERPROFILE%/.config/zai/key.json`; newly entered keys use protected OpenUsage storage | No environment name or compatibility file was present | Validate protected save/status/delete flow in P4 |

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
- WSL distributions are outside this matrix. No fallback crosses into WSL automatically.
- Real-provider verification records only source kind, outcome category, and normalized response shape.
  Tokens, cookies, usernames, tenant identifiers, raw rows, and full paths are not captured.
