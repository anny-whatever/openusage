# OpenUsage for Windows

This workspace is the native Windows 11 x64 application. It uses a Rust/Tauri backend for all trusted
local and provider work and a React WebView for presentation only. See `docs/windows-architecture.md`
and `docs/windows-provider-support.md` for the platform boundary and provider-source status.

## Prerequisites

User-local tools:

- Rust 1.95.0, installed with rustup for `x86_64-pc-windows-msvc`, including rustfmt and Clippy.
- Bun 1.3.14.

Administrator-installed tools:

- Visual Studio Build Tools 2022 with `Microsoft.VisualStudio.Component.VC.Tools.x86.x64`.
- Windows 11 SDK 10.0.26100 or newer.
- Microsoft Edge WebView2 Evergreen Runtime.

Verify versions from PowerShell or a Developer PowerShell for Visual Studio:

```powershell
bun --version
rustc --version
cargo --version
cargo clippy --version
rustfmt --version
Get-ItemProperty 'HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\*' |
    Where-Object name -Match 'WebView2' |
    Select-Object name, pv
```

The C++ compiler and SDK are available after entering a Developer PowerShell:

```powershell
cl
Get-Command msbuild
Get-Command rc
```

## Install

Dependency versions are exact in `package.json` and `Cargo.toml`; `bun.lock` and `Cargo.lock` lock the
full graphs.

```powershell
bun install --frozen-lockfile
```

Do not run a persistent development server for routine verification. Build the frontend and restart the
long-lived tray process after every source change.

## Quality Gates

```powershell
bun run typecheck
bun run lint
bun run test
bun run build
Set-Location src-tauri
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
```

When the repository is checked out inside WSL, Windows Cargo cannot place executable build artifacts on
the WSL UNC filesystem. The wrapper imports the real MSVC environment and directs artifacts to bounded
Windows-local build storage:

```bash
script/wsl-windows-cargo.sh fmt -- --check
script/wsl-windows-cargo.sh test
script/wsl-windows-cargo.sh clippy --all-targets -- -D warnings
script/wsl-windows-cargo.sh build --release
```

The real-host smoke probe launches the release executable with its explicit `--show` diagnostic option,
verifies a visible focused `OpenUsage` window, records bounded process resource counts, and always
terminates the probe process. Its `-VerifyQuit` mode uses the same graceful exit function as the tray
menu and proves that the process terminates without an orphan:

```powershell
powershell -ExecutionPolicy Bypass -File script/verify-native-launch.ps1
powershell -ExecutionPolicy Bypass -File script/verify-native-launch.ps1 -VerifyQuit
```

The stable shadcn registry lives as editable source under `src/components/ui`. The experimental
message-scroller item is intentionally excluded because shadcn 4.13.0 implements it through the
`@shadcn/react` runtime package, which would violate this workspace's source-owned primitive boundary.
