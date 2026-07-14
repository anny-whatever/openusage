#!/usr/bin/env bash

set -euo pipefail

readonly script_directory=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$script_directory/../src-tauri"

if [[ $# -eq 0 ]]; then
  echo "Usage: $0 <cargo-subcommand> [arguments...]" >&2
  exit 2
fi

readonly vswhere_path="${VSWHERE_PATH:-/mnt/c/Program Files (x86)/Microsoft Visual Studio/Installer/vswhere.exe}"
if [[ ! -x "$vswhere_path" ]]; then
  echo "Visual Studio Installer discovery tool was not found at: $vswhere_path" >&2
  exit 1
fi

visual_studio_path=$(
  "$vswhere_path" \
    -latest \
    -products '*' \
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 \
    -property installationPath |
    tr -d '\r'
)

if [[ -z "$visual_studio_path" ]]; then
  echo "Visual Studio C++ build tools were not found." >&2
  exit 1
fi

if [[ "$visual_studio_path" == *" "* ]]; then
  echo "The WSL wrapper requires a Visual Studio installation path without spaces." >&2
  echo "Run Cargo from a Developer PowerShell for Visual Studio on a native Windows checkout instead." >&2
  exit 1
fi

visual_studio_environment=$(
  cmd.exe /d /c \
    "call $visual_studio_path\Common7\Tools\VsDevCmd.bat -arch=x64 -host_arch=x64 >nul && set" \
    2>/dev/null |
    tr -d '\r'
)

read_environment_value() {
  local variable_name="$1"
  printf '%s\n' "$visual_studio_environment" |
    sed -n "s/^${variable_name}=//p" |
    tail -1
}

windows_path=$(read_environment_value "Path")
include_path=$(read_environment_value "INCLUDE")
library_path=$(read_environment_value "LIB")
library_search_path=$(read_environment_value "LIBPATH")
target_directory=$(
  cmd.exe /d /c "echo %LOCALAPPDATA%\OpenUsage\build-target" 2>/dev/null |
    tr -d '\r' |
    tail -1
)

if [[ -z "$windows_path" || -z "$include_path" || -z "$library_path" ]]; then
  echo "The Visual Studio compiler environment was incomplete." >&2
  exit 1
fi

windows_user_profile=$(
  cmd.exe /d /c "echo %USERPROFILE%" 2>/dev/null |
    tr -d '\r' |
    tail -1
)
default_cargo_path=$(wslpath -u "$windows_user_profile\.cargo\bin\cargo.exe")
readonly cargo_path="${WINDOWS_CARGO_PATH:-$default_cargo_path}"
if [[ ! -x "$cargo_path" ]]; then
  echo "Windows Cargo was not found at: $cargo_path" >&2
  exit 1
fi

if [[ "$1" == "fmt" ]]; then
  cargo_arguments=("$@")
else
  cargo_arguments=("$1" --target-dir "$target_directory" "${@:2}")
fi

env \
  "PATH=$windows_path" \
  "INCLUDE=$include_path" \
  "LIB=$library_path" \
  "LIBPATH=$library_search_path" \
  "$cargo_path" "${cargo_arguments[@]}"
