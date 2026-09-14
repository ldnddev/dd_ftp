#!/usr/bin/env bash
# install.sh — install dd_ftp for this machine.
#
# Quick install (no checkout required):
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_ftp/master/install.sh | bash
#
# From a git checkout:
#   ./install.sh                 # release cargo build (developer default)
#   ./install.sh --from-release  # prebuilt GitHub package
#   ./install.sh --uninstall
set -euo pipefail

if [[ -z "${BASH_VERSION:-}" ]]; then
  printf 'Error: please run this script with bash.\n' >&2
  printf '  curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_ftp/master/install.sh | bash\n' >&2
  exit 1
fi

REPO="${REPO:-ldnddev/dd_ftp}"
REPO_URL="${REPO_URL:-https://github.com/${REPO}.git}"
APP_NAME="dd_ftp"
THEME_FILE="dd_ftp_theme.yml"
BIN_DST_NAME="${BIN_NAME:-dd_ftp}"
VERSION="${VERSION:-latest}"
INSTALL_DIR="${INSTALL_DIR:-${HOME}/.local/bin}"
CONFIG_DIR="${CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/ldnddev}"

FROM_RELEASE=0
FROM_SOURCE=0
UNINSTALL=0
DRY_RUN=0
PRINT_TARGET=0
BRANCH=""

usage() {
  cat <<USAGE
Install ${APP_NAME} for this machine.

Usage:
  curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_ftp/master/install.sh | bash
  ./install.sh [options]

Options:
  --from-release      Download a prebuilt GitHub release package
  --from-source       Build from source with cargo
  --version TAG       Release tag to install (default: latest)
  --branch NAME       Git branch to clone when building from source
  --prefix DIR        Install the binary into DIR (same as INSTALL_DIR)
  --uninstall         Remove the installed binary and default theme
  --print-target      Print the detected package target and exit
  --dry-run           Show what would be installed, then exit
  -h, --help          Show this help

Environment:
  INSTALL_DIR         Binary directory (default: ~/.local/bin)
  BIN_NAME            Installed binary name (default: dd_ftp)
  VERSION             Release tag or "latest"
  REPO                GitHub owner/name (default: ldnddev/dd_ftp)
  CONFIG_DIR          Theme directory (default: ~/.config/ldnddev)

Examples:
  INSTALL_DIR=/usr/local/bin ./install.sh
  curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_ftp/master/install.sh | bash -s -- --version v1.2.0
USAGE
}

CLEANUP_PATHS=()
cleanup() {
  local p
  if [[ ${#CLEANUP_PATHS[@]} -eq 0 ]]; then
    return 0
  fi
  for p in "${CLEANUP_PATHS[@]}"; do
    rm -rf "$p"
  done
}
trap cleanup EXIT

fail() {
  printf 'Error: %s\n' "$*" >&2
  exit 1
}

info() {
  printf '==> %s\n' "$*"
}

warn() {
  printf 'Warning: %s\n' "$*" >&2
}

have() {
  command -v "$1" >/dev/null 2>&1
}

script_dir() {
  local src=""
  # `curl | bash` leaves BASH_SOURCE unset. With `set -u`, `${BASH_SOURCE[0]}`
  # is an unbound variable on some bash versions even with `${...:-}`.
  set +u
  src="${BASH_SOURCE[0]}"
  set -u
  [[ -n "$src" && "$src" != "-" && "$src" != "bash" && "$src" == *install.sh && -f "$src" ]] || return 1
  (cd "$(dirname "$src")" && pwd)
}

is_local_checkout() {
  local dir
  dir="$(script_dir)" || return 1
  [[ -f "$dir/Cargo.toml" && -d "$dir/crates/dd_ftp_cli" ]]
}

detect_os() {
  local os
  os="$(uname -s | tr '[:upper:]' '[:lower:]')"
  case "$os" in
    linux) printf 'linux' ;;
    darwin) printf 'darwin' ;;
    mingw* | msys* | cygwin*) printf 'windows' ;;
    *) fail "unsupported OS: $(uname -s)" ;;
  esac
}

detect_arch() {
  local arch
  arch="$(uname -m | tr '[:upper:]' '[:lower:]')"
  case "$arch" in
    x86_64 | amd64) printf 'x86_64' ;;
    arm64 | aarch64) printf 'aarch64' ;;
    *) fail "unsupported architecture: $(uname -m)" ;;
  esac
}

detect_target() {
  local os arch
  os="$(detect_os)"
  arch="$(detect_arch)"
  case "$os" in
    linux) printf '%s-unknown-linux-gnu' "$arch" ;;
    darwin) printf '%s-apple-darwin' "$arch" ;;
    windows) printf '%s-pc-windows-msvc' "$arch" ;;
  esac
}

normalize_version() {
  local v="$1"
  [[ "$v" == "latest" ]] && { printf 'latest'; return; }
  [[ "$v" == v* ]] && { printf '%s' "$v"; return; }
  printf 'v%s' "$v"
}

package_url() {
  local target="$1"
  local version="$2"
  local base="https://github.com/${REPO}/releases"
  if [[ "$version" == "latest" ]]; then
    printf '%s/latest/download/dd_ftp-%s.tar.gz' "$base" "$target"
  else
    printf '%s/download/%s/dd_ftp-%s.tar.gz' "$base" "$version" "$target"
  fi
}

checksums_url() {
  local version="$1"
  local base="https://github.com/${REPO}/releases"
  if [[ "$version" == "latest" ]]; then
    printf '%s/latest/download/SHA256SUMS' "$base"
  else
    printf '%s/download/%s/SHA256SUMS' "$base" "$version"
  fi
}

download() {
  local url="$1"
  local dest="$2"
  if have curl; then
    curl -fsSL --retry 3 --retry-delay 1 --connect-timeout 15 -o "$dest" "$url"
  elif have wget; then
    wget -q -O "$dest" "$url"
  else
    fail "curl or wget is required to download packages"
  fi
}

download_optional() {
  local url="$1"
  local dest="$2"
  if have curl; then
    curl -fsSL --retry 3 --retry-delay 1 --connect-timeout 15 -o "$dest" "$url" 2>/dev/null
  elif have wget; then
    wget -q -O "$dest" "$url" 2>/dev/null
  else
    return 1
  fi
}

file_sha256() {
  if have sha256sum; then
    sha256sum "$1" | awk '{print $1}'
  elif have shasum; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    return 1
  fi
}

verify_checksum() {
  local archive="$1"
  local sums="$2"
  local name expected actual
  [[ -f "$sums" ]] || return 1
  name="$(basename "$archive")"
  expected="$(awk -v f="$name" '$2 == f || $2 == ("*" f) {print $1; exit}' "$sums")"
  [[ -n "$expected" ]] || fail "no checksum entry for ${name}"
  actual="$(file_sha256 "$archive")" || fail "could not hash ${name}"
  [[ "$expected" == "$actual" ]] || fail "checksum mismatch for $name"
  info "checksum ok"
}

install_file() {
  local src="$1"
  local dest="$2"
  local mode="$3"
  mkdir -p "$(dirname "$dest")"
  if have install; then
    install -m "$mode" "$src" "$dest"
  else
    cp "$src" "$dest"
    chmod "$mode" "$dest"
  fi
}

install_theme_if_present() {
  local src="$1"
  local dest="${CONFIG_DIR}/${THEME_FILE}"
  [[ -f "$src" ]] || return 0
  mkdir -p "$CONFIG_DIR"
  if [[ -f "$dest" ]]; then
    info "theme already exists at ${dest} (left unchanged)"
    return 0
  fi
  install_file "$src" "$dest" 0644
  info "installed default theme ${dest}"
}

path_note() {
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
      printf '\nNote: %s is not currently in PATH. Add this to your shell config:\n' "$INSTALL_DIR"
      printf '  export PATH="%s:$PATH"\n' "$INSTALL_DIR"
      ;;
  esac
}

installed_bin_path() {
  local os
  os="$(detect_os)"
  if [[ "$os" == "windows" ]]; then
    printf '%s/%s.exe' "$INSTALL_DIR" "$BIN_DST_NAME"
  else
    printf '%s/%s' "$INSTALL_DIR" "$BIN_DST_NAME"
  fi
}

do_uninstall() {
  local bin theme
  bin="$(installed_bin_path)"
  theme="${CONFIG_DIR}/${THEME_FILE}"

  if [[ -e "$bin" || -L "$bin" ]]; then
    rm -f "$bin"
    info "removed ${bin}"
  else
    info "binary not found at ${bin}"
  fi

  if [[ -e "$theme" || -L "$theme" ]]; then
    rm -f "$theme"
    info "removed ${theme}"
  else
    info "theme not found at ${theme}"
  fi

  if [[ -d "$CONFIG_DIR" ]] && rmdir "$CONFIG_DIR" 2>/dev/null; then
    info "removed empty ${CONFIG_DIR}"
  fi

  info "uninstall complete"
}

extract_package() {
  local archive="$1"
  local dest="$2"
  mkdir -p "$dest"
  tar -xzf "$archive" -C "$dest"
}

find_extracted_bin() {
  local dir="$1"
  local os="$2"
  local candidate
  if [[ "$os" == "windows" ]]; then
    for candidate in \
      "${dir}/dd_ftp.exe" \
      "${dir}/dd_ftp_cli.exe" \
      "${dir}/bin/dd_ftp.exe" \
      "${dir}/bin/dd_ftp_cli.exe"
    do
      if [[ -f "$candidate" ]]; then
        printf '%s' "$candidate"
        return 0
      fi
    done
  else
    for candidate in \
      "${dir}/dd_ftp" \
      "${dir}/dd_ftp_cli" \
      "${dir}/bin/dd_ftp" \
      "${dir}/bin/dd_ftp_cli"
    do
      if [[ -f "$candidate" ]]; then
        printf '%s' "$candidate"
        return 0
      fi
    done
  fi
  return 1
}

install_from_release() {
  local target version url archive tmp sums_url extracted src_bin dest theme
  target="$(detect_target)"
  version="$(normalize_version "$VERSION")"
  url="${DD_FTP_PACKAGE_URL:-$(package_url "$target" "$version")}"
  dest="$(installed_bin_path)"

  info "detected target ${target}"
  info "downloading ${url}"

  tmp="$(mktemp -d)"
  CLEANUP_PATHS+=("$tmp")

  archive="${tmp}/dd_ftp-${target}.tar.gz"
  if ! download "$url" "$archive" 2>/dev/null; then
    return 1
  fi

  sums_url="$(checksums_url "$version")"
  if download_optional "$sums_url" "${tmp}/SHA256SUMS"; then
    verify_checksum "$archive" "${tmp}/SHA256SUMS"
  else
    warn "SHA256SUMS not found; skipping checksum verification"
  fi

  extracted="${tmp}/pkg"
  extract_package "$archive" "$extracted"
  src_bin="$(find_extracted_bin "$extracted" "$(detect_os)")" \
    || fail "package did not contain a dd_ftp binary"

  install_file "$src_bin" "$dest" 0755
  info "installed ${dest}"

  theme="${extracted}/${THEME_FILE}"
  if [[ ! -f "$theme" && -f "${extracted}/bin/${THEME_FILE}" ]]; then
    theme="${extracted}/bin/${THEME_FILE}"
  fi
  if [[ -f "$theme" ]]; then
    install_theme_if_present "$theme"
  fi
  return 0
}

build_and_install_from() {
  local src_dir="$1"
  local dest src_bin
  dest="$(installed_bin_path)"

  have cargo || fail "cargo is required to build from source. Install Rust from https://rustup.rs/"

  info "building dd_ftp (release) from ${src_dir}"
  cargo build --release -p dd_ftp_cli --manifest-path "${src_dir}/Cargo.toml"

  if [[ "$(detect_os)" == "windows" ]]; then
    src_bin="${src_dir}/target/release/dd_ftp_cli.exe"
  else
    src_bin="${src_dir}/target/release/dd_ftp_cli"
  fi
  [[ -f "$src_bin" ]] || fail "built binary not found at ${src_bin}"

  install_file "$src_bin" "$dest" 0755
  info "installed ${dest}"
  install_theme_if_present "${src_dir}/${THEME_FILE}"
}

install_from_source() {
  local src_dir workdir clone_args
  if is_local_checkout; then
    src_dir="$(script_dir)"
    build_and_install_from "$src_dir"
    return 0
  fi

  have git || fail "git is required to clone the source when no prebuilt package is available"

  workdir="$(mktemp -d)"
  CLEANUP_PATHS+=("$workdir")
  info "cloning ${REPO_URL}"
  clone_args=(--depth 1)
  if [[ -n "$BRANCH" ]]; then
    clone_args+=(--branch "$BRANCH")
  fi
  GIT_TERMINAL_PROMPT=0 git clone "${clone_args[@]}" "$REPO_URL" "${workdir}/src"
  build_and_install_from "${workdir}/src"
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --from-release) FROM_RELEASE=1; shift ;;
    --from-source) FROM_SOURCE=1; shift ;;
    --uninstall | -uninstall) UNINSTALL=1; shift ;;
    --print-target) PRINT_TARGET=1; shift ;;
    --dry-run) DRY_RUN=1; shift ;;
    --version)
      VERSION="${2:-}"
      [[ -n "$VERSION" ]] || fail "--version requires a tag"
      shift 2
      ;;
    --version=*)
      VERSION="${1#*=}"
      shift
      ;;
    --branch)
      BRANCH="${2:-}"
      [[ -n "$BRANCH" ]] || fail "--branch requires a name"
      shift 2
      ;;
    --prefix)
      INSTALL_DIR="${2:-}"
      [[ -n "$INSTALL_DIR" ]] || fail "--prefix requires a directory"
      shift 2
      ;;
    --help | -h)
      usage
      exit 0
      ;;
    *)
      fail "unknown option: $1 (try --help)"
      ;;
  esac
done

if [[ "$PRINT_TARGET" -eq 1 ]]; then
  detect_target
  printf '\n'
  exit 0
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
  printf 'target:      %s\n' "$(detect_target)"
  printf 'package:     %s\n' "$(package_url "$(detect_target)" "$(normalize_version "$VERSION")")"
  printf 'install to:  %s\n' "$(installed_bin_path)"
  printf 'theme:       %s/%s\n' "$CONFIG_DIR" "$THEME_FILE"
  if is_local_checkout; then
    printf 'checkout:    %s\n' "$(script_dir)"
  else
    printf 'checkout:    (none; curl/piped install)\n'
  fi
  exit 0
fi

if [[ "$UNINSTALL" -eq 1 ]]; then
  do_uninstall
  exit 0
fi

if [[ "$FROM_RELEASE" -eq 1 && "$FROM_SOURCE" -eq 1 ]]; then
  fail "use either --from-release or --from-source, not both"
fi

prefer_source=0
if [[ "$FROM_SOURCE" -eq 1 ]]; then
  prefer_source=1
elif [[ "$FROM_RELEASE" -eq 1 ]]; then
  prefer_source=0
elif is_local_checkout && have cargo; then
  prefer_source=1
fi

if [[ "$prefer_source" -eq 1 ]]; then
  install_from_source
else
  if install_from_release; then
    :
  elif [[ "$FROM_RELEASE" -eq 1 ]]; then
    fail "no prebuilt package for $(detect_target). Check https://github.com/${REPO}/releases"
  else
    warn "no prebuilt package for $(detect_target); building from source"
    install_from_source
  fi
fi

path_note
printf '\nRun: %s\n' "$BIN_DST_NAME"
