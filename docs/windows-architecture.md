# Windows Architecture

This document records the implementation boundary for the Windows edition of OpenUsage. The Windows
edition is a separate platform surface in this repository, not a conditional compilation mode for the
existing Swift application.

## Decision

The Windows application uses Tauri 2 with a Rust backend and a React/TypeScript frontend under
`windows/`.

- Rust owns credentials, files, SQLite, processes, network requests, provider refreshes, caching,
  normalized usage models, the loopback API, logging, and update installation.
- React owns presentation, local interaction state, keyboard navigation, and accessibility. It receives
  only normalized, secret-free data through typed commands and events.
- The macOS Swift application remains the active macOS implementation. Production Swift behavior is not
  routed through the Windows application.
- The frozen `tauri-legacy` branch is reference material only. Its macOS `NSPanel`, Keychain host API,
  QuickJS plugin runtime, and retired `0.6.x` update channel are not restored.

This avoids two unsafe alternatives: exposing provider credentials to a WebView, or maintaining an
arbitrary JavaScript plugin host with broad filesystem, process, keychain, and network authority.

## Support Boundary

The first Windows beta targets Windows 11 x64 and native Windows installations of provider tools.

- Windows 10 is outside the initial support floor because Microsoft ended general support in 2025.
- Windows ARM64 follows only after x64 behavior, packaging, and updater paths are stable.
- WSL credential and log discovery is separate work. The first edition never walks `\\wsl$` or
  `\\wsl.localhost` implicitly.
- iCloud Sync is unavailable on Windows. It is not replaced with an unverified shared folder. A future
  cross-platform sync design requires its own threat model and conflict protocol.

## Repository Shape

The Windows workspace follows feature ownership rather than one large UI or backend file.

- `windows/src/` contains the React application shell, features, shared UI primitives, and typed IPC
  client.
- `windows/src-tauri/src/` contains the Rust composition root, platform adapters, providers, stores,
  services, and Tauri lifecycle.
- `windows/src-tauri/capabilities/` contains the explicit WebView permission boundary.
- `Tests/Fixtures/ProviderParity/` contains language-neutral provider inputs and normalized outputs
  consumed by both Swift and Rust tests.

Files stay below 500 lines, React components stay below 200 lines, and runtime modules have one clear
responsibility.

## Trusted Boundary

The bundled WebView is untrusted relative to local credentials, even though it loads only bundled code.

The frontend may request or receive:

- normalized provider snapshots and stable error categories;
- provider and metric descriptors;
- secret-free API-key status such as missing, saved, or environment override;
- persisted layout and display preferences;
- narrowly scoped actions such as refresh, open a known URL, save a newly entered key, or clear a key.

The frontend never receives stored tokens, cookies, raw credential files, raw SQLite rows, provider
response bodies, process command lines, proxy credentials, or updater private material. A save-key
command accepts the new value once, writes it through the protected backend adapter, zeroizes its owned
buffer where practical, and returns status only.

Every command validates identifiers and payload size at the Rust boundary. Tauri capabilities are
restricted to the `main` window, remote content is not granted IPC access, and the Content Security
Policy permits bundled assets plus the minimum Tauri IPC endpoints only.

## Runtime Ownership and Bounds

The Rust composition root owns every long-lived task and cancels it during shutdown.

- Provider refreshes are coalesced per provider and use a fixed concurrency limit.
- Every external request and subprocess has a timeout and cancellation path.
- Subprocess stdout and stderr are drained concurrently into capped buffers; timeouts terminate the
  Windows process tree.
- The snapshot cache stores one latest normalized snapshot per provider.
- Usage history is limited to the product's fixed reporting windows; append-only logs are scanned from
  persisted checkpoints rather than retained in memory.
- The loopback server retains at most 16 active connections and at most 8 KiB of each request head.
- Logs rotate at a fixed size and pass through URL, body, token, cookie, path, and identifier redaction.
- UI countdowns use one shared scheduler rather than one timer per metric.

The expected refresh path is `O(P + M + delta-log-rows)`, where `P` is the provider count and `M` is the
visible metric count. Retained memory is `O(P * M)` plus fixed 30-day history and explicitly bounded
caches.

## P2 Runtime Foundation

The first trusted runtime layer is implemented below `windows/src-tauri/src/`:

- `contracts/` owns the `openusage.provider-snapshot.v1` and `openusage.limits.v1` wire schemas. It
  validates identifiers, timestamps, day keys, duplicate resources and metrics, progress ranges, and
  every numeric boundary before normalized data is persisted or exposed.
- `platform/` owns native environment and path discovery, same-directory atomic replacement, DPAPI
  secret protection, WAL-aware read-only SQLite snapshots, bounded subprocess execution, asynchronous
  HTTP with system or explicit proxy support, and redacting two-file log rotation.
- `runtime/` owns versioned settings migration, one-snapshot-per-provider persistence, five-minute
  session freshness, stale-while-revalidate display, fixed failure backoff, first-run credential probes,
  coalesced per-provider refreshes, and a global refresh semaphore.

Persisted snapshots paint immediately after launch but never count as fresh in a new process. The first
refresh of every enabled provider therefore runs once per session; subsequent refreshes reuse a
successful snapshot for five minutes. Failed snapshots are not persisted, missing history on a later
successful response preserves the last successful history, and wake notifications use a one-item
channel so repeated resume or settings events cannot create an unbounded queue.

## Storage and Credentials

Application state lives below the Tauri-resolved local application data directory and is written through
same-directory temporary files followed by an atomic replace. Settings and cache payloads carry explicit
schema versions.

OpenUsage-owned API keys use Windows Credential Manager or DPAPI-backed storage. Compatibility config
files and environment variables remain read sources where documented, but new secrets are never written
to plaintext JSON. Third-party credential stores are read only when their exact Windows target naming
and access behavior have been verified; absence and access denial are distinct results.

SQLite sources open read-only and must not create, migrate, checkpoint, or otherwise mutate another
application's database. WAL-backed sources require a tested snapshot strategy that includes the matching
WAL state and cleans private temporary copies on every exit path.

## Parity Strategy

The Swift and Rust implementations share behavior contracts, not executable code or FFI.

For each provider, language-neutral fixtures capture external response shapes and the expected normalized
snapshot. Both implementations must pass the same fixtures for values, units, reset windows, missing
fields, errors, histories, and malformed boundaries. Real Windows verification supplements fixtures for
credential discovery, file locking, process discovery, and OS integration.

Provider differences are documented rather than hidden. A macOS-only source can be absent on Windows
while the provider remains supported through another verified source; if no source exists, the Windows UI
shows an explicit platform limitation.

## Packaging and Updates

Windows ships as a per-user, Authenticode-signed NSIS installer. Tauri updater bundles use a separate
update-signing key and Windows-specific stable and beta manifests. They never overwrite Sparkle's
`appcast.xml` or the frozen legacy `latest.json` preserved for `0.6.28` clients.

Version selection, tag creation, signing-secret configuration, upload, and publication remain explicit
owner actions.

## Pinned P1 Toolchain

| Tool | Version |
| --- | --- |
| Rust and Cargo | 1.95.0 |
| Bun | 1.3.14 |
| Tauri Rust | 2.11.2 |
| Tauri CLI | 2.11.4 |
| Tauri JavaScript API | 2.11.1 |
| React | 19.2.7 |
| TypeScript | 6.0.3 |
| Vite | 8.1.4 |
| shadcn CLI | 4.13.0 |
| Windows SDK | 10.0.26100 or newer |

The repository lockfiles are the source of truth for every transitive dependency.
