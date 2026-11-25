#!/usr/bin/env bash
# install.sh — install dd_ftp for this machine.
#
# Quick install (no checkout required):
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_ftp/master/install.sh | bash
#   curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_ftp/master/install.sh | bash -s -- --both
#
# From a git checkout:
#   ./install.sh                 # ask: terminal, window, or both
#   ./install.sh --tui           # terminal only (cargo -p dd_ftp_cli, not --workspace)
#   ./install.sh --gui           # window only (cargo -p dd_ftp_gui)
#   ./install.sh --both
#   ./install.sh --from-release  # prebuilt GitHub package (terminal archive is GNU)
#   ./install.sh --uninstall     # ask which binary to remove; leaves dd_ftp_theme.yml
#
# INSTALL_GUI=1 with no --tui/--gui/--both selects both.
# BIN_DIR and PREFIX are aliases for the binary directory. INSTALL_DIR wins.
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
GUI_BIN_NAME="dd_ftp-gui"
GUI_APP_ID="dev.ldnddev.dd_ftp"
VERSION="${VERSION:-latest}"
# INSTALL_DIR wins. Otherwise BIN_DIR, then PREFIX/bin, then ~/.local/bin.
if [[ -z "${INSTALL_DIR+x}" ]]; then
  if [[ -n "${BIN_DIR:-}" ]]; then
    INSTALL_DIR="$BIN_DIR"
  elif [[ -n "${PREFIX:-}" ]]; then
    INSTALL_DIR="${PREFIX%/}/bin"
  else
    INSTALL_DIR="${HOME}/.local/bin"
  fi
fi
CONFIG_DIR="${CONFIG_DIR:-${XDG_CONFIG_HOME:-$HOME/.config}/ldnddev}"

FROM_RELEASE=0
FROM_SOURCE=0
UNINSTALL=0
DRY_RUN=0
PRINT_TARGET=0
BRANCH=""
TARGET_CHOICE=""

usage() {
  cat <<USAGE
Install ${APP_NAME} for this machine.

Usage:
  curl -fsSL https://raw.githubusercontent.com/ldnddev/dd_ftp/master/install.sh | bash
  ./install.sh [--tui | --gui | --both] [options]

  ./install.sh
      ask: terminal app, desktop window, or both
  ./install.sh --tui
      install the terminal (dd_ftp)
  ./install.sh --gui
      install the window (dd_ftp-gui)
  ./install.sh --both
      install both
  ./install.sh --uninstall
      ask which of those to remove

With no terminal to answer, install selects the terminal app and uninstall
removes both. INSTALL_GUI=1 is the same as --both.

Options:
  --tui               Terminal app only
  --gui               Desktop window only
  --both              Terminal and window
  --from-release      Download a prebuilt GitHub release package
  --from-source       Build from source with cargo
  --version TAG       Release tag to install (default: latest)
  --branch NAME       Git branch to clone when building from source
  --prefix DIR        Install binaries into DIR (same as INSTALL_DIR)
  --uninstall         Remove the chosen binary and its launcher
  --print-target      Print the detected package target and exit
  --dry-run           Show what would be installed, then exit
  --self-test         Check archive, checksum, and launcher handling, then exit
  -h, --help          Show this help

Environment:
  INSTALL_DIR         Binary directory (default: ~/.local/bin)
  BIN_DIR             Alias for INSTALL_DIR when INSTALL_DIR is unset
  PREFIX              Alias: \$PREFIX/bin when INSTALL_DIR and BIN_DIR are unset
  BIN_NAME            Installed terminal name (default: dd_ftp)
  INSTALL_GUI         Set to 1 to select both when no --tui/--gui/--both
  VERSION             Release tag or "latest"
  REPO                GitHub owner/name (default: ldnddev/dd_ftp)
  CONFIG_DIR          Theme directory (default: ~/.config/ldnddev)
  XDG_DATA_HOME       Linux launcher parent when the binary stays under \$HOME

Uninstall removes only the chosen binary and its .desktop file.
dd_ftp_theme.yml, app config, bookmarks, and keyring secrets stay.
The window needs Vulkan (libvulkan1 and a driver) and Wayland or X11.

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

# ldnddev wordmark. Printed only on a terminal so a captured log stays plain text.
print_mark() {
  [[ -t 1 ]] || return 0
  printf '\033[40;97m'
  cat <<'EOF'

█▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀█
█  ██                ██                         ██  █
█  ██                ██                         ██  █
█  ██         ▄▄▄▄▄▄ ██     ▄▄▄▄▄▄       ▄▄▄▄▄▄ ██  █
█  ██       ▄██▀▀▀▀▀███  ▄██████████▄  ▄██▀▀▀▀▀███  █
█  ██      ██▀       ██  ██        ██ ██▀       ██  █
█  ██      ██        ██  ██        ██ ██        ██  █
█  ██      ▀█▄       ██  ██        ██ ▀█▄       ██  █
█  ▀█▄▄▄▄▄  ▀██▄▄▄▄██▀   ██        ██  ▀██▄▄▄▄██▀   █
█   ▀▀▀▀▀▀    ▀▀▀▀▀▀     ▀▀        ▀▀    ▀▀▀▀▀▀     █
█                                                   █
█                     ██                            █
█                     ██                            █
█              ▄▄▄▄▄▄ ██     ▄▄▄▄▄▄    ▄▄      ▄▄▄  █
█            ▄██▀▀▀▀▀███   ██▀▀▀▀███   ██     ▄██   █
█           ██▀       ██  ██    ██▀ ▄▄ ██    ▄██    █
█           ██        ██  ██  ▄██   ██ ██    ██     █
█           ▀█▄       ██  ██▄██▀   ▄█▀ ██  ▄██      █
█  ▄▄▄▄▄▄▄▄  ▀██▄▄▄▄██▀    ▀███▄▄▄██▀  ██▄██▀       █
█  ▀▀▀▀▀▀▀▀    ▀▀▀▀▀▀        ▀▀▀▀▀▀    ▀▀▀▀         █
▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
EOF
  printf '\033[0m\n'
}

# The device node is mode rw-rw-rw, so -r/-w stay true with no controlling
# terminal. Opening it is the check that matters.
have_tty() {
  { : <>/dev/tty; } 2>/dev/null
}

# Read 1, 2, or 3 from the terminal. stdout is tui, gui, or both.
# $1 is the verb. $2 is the default number.
# Returns 1 when the terminal cannot be used. main captures stdout, and a
# failed redirect in that capture does not abort under set -e, so every
# /dev/tty failure has to be an explicit status.
ask_choice() {
  local verb=$1 default=$2 answer choice
  while true; do
    {
      printf '%s which program?\n' "$verb"
      printf '%s\n' "  1) terminal (dd_ftp)"
      printf '%s\n' "  2) desktop window (dd_ftp-gui)"
      printf '%s\n' "  3) both"
      printf 'Choice [%s]: ' "$default"
    } >/dev/tty 2>/dev/null || return 1
    if ! IFS= read -r answer </dev/tty 2>/dev/null; then
      printf '\n' >/dev/tty 2>/dev/null || true
      answer=$default
    fi
    choice=$(printf '%s' "$answer" | tr '[:upper:]' '[:lower:]' | tr -d '[:space:]')
    case "$choice" in
      '' ) choice=$default ;;
    esac
    case "$choice" in
      1 | tui | terminal) printf '%s\n' tui; return 0 ;;
      2 | gui | window) printf '%s\n' gui; return 0 ;;
      3 | both) printf '%s\n' both; return 0 ;;
      *) printf '%s\n' "Enter 1, 2, or 3." >/dev/tty 2>/dev/null || return 1 ;;
    esac
  done
}

# $1 is install or uninstall. $2 is an explicit tui/gui/both, or empty.
# stdout is only the choice. Notices go to stderr so a capture stays one word.
choose_target() {
  local action=$1 explicit=$2
  if [[ -n "$explicit" ]]; then
    printf '%s\n' "$explicit"
    return 0
  fi
  if have_tty; then
    if [[ "$action" == uninstall ]]; then
      ask_choice "Remove" 3 && return 0
    else
      ask_choice "Install" 1 && return 0
    fi
  fi
  if [[ "$action" == uninstall ]]; then
    printf '%s\n' "No terminal for a prompt. Removing both programs." >&2
    printf '%s\n' both
  else
    printf '%s\n' "No terminal for a prompt. Installing the terminal app." >&2
    printf '%s\n' tui
  fi
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

# Separate GNU archive. It does not replace the terminal package above.
gui_package_url() {
  local target="$1"
  local version="$2"
  local base="https://github.com/${REPO}/releases"
  if [[ "$version" == "latest" ]]; then
    printf '%s/latest/download/dd_ftp-gui-%s.tar.gz' "$base" "$target"
  else
    printf '%s/download/%s/dd_ftp-gui-%s.tar.gz' "$base" "$version" "$target"
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

# True when a tar member is absolute or has ".." as its own path component.
# "foo..bar" is a normal name. GNU tar strips some of these on extract; other
# tar implementations do not, so the archive is rejected before extract.
archive_member_is_unsafe() {
  local member="$1"
  local part rest
  case "$member" in
    /* | [A-Za-z]:* | \\*) return 0 ;;
  esac
  rest="$member"
  while [[ -n "$rest" ]]; do
    part="${rest%%/*}"
    if [[ "$part" == ".." ]]; then
      return 0
    fi
    if [[ "$rest" == */* ]]; then
      rest="${rest#*/}"
    else
      break
    fi
  done
  return 1
}

reject_unsafe_archive_members() {
  local archive="$1"
  local member listing
  listing="$(tar -tzf "$archive" 2>/dev/null)" || {
    printf 'Error: could not list members of %s\n' "$(basename "$archive")" >&2
    return 1
  }
  while IFS= read -r member || [[ -n "$member" ]]; do
    [[ -n "$member" ]] || continue
    if archive_member_is_unsafe "$member"; then
      printf 'Error: refusing archive member %s\n' "$member" >&2
      return 1
    fi
  done <<<"$listing"
  return 0
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

installed_gui_path() {
  local os
  os="$(detect_os)"
  if [[ "$os" == "windows" ]]; then
    printf '%s/%s.exe' "$INSTALL_DIR" "$GUI_BIN_NAME"
  else
    printf '%s/%s' "$INSTALL_DIR" "$GUI_BIN_NAME"
  fi
}

gui_desktop_path() {
  printf '%s/%s.desktop\n' "$(apps_dir)" "$GUI_BIN_NAME"
}

# Menus look in $XDG_DATA_HOME/applications, then the system data dirs.
# A binary under $HOME is published where the session looks. A binary
# outside $HOME (for example /usr/local/bin) is published under that
# prefix's share/applications.
apps_dir() {
  local dir prefix
  dir="${INSTALL_DIR%/}"
  case "$dir" in
    "$HOME" | "$HOME"/*)
      if [[ -n "${XDG_DATA_HOME:-}" ]]; then
        printf '%s/applications\n' "${XDG_DATA_HOME%/}"
      else
        printf '%s/.local/share/applications\n' "$HOME"
      fi
      ;;
    */bin)
      prefix="${dir%/bin}"
      if [[ -z "$prefix" || "$prefix" == "/" ]]; then
        printf '/share/applications\n'
      else
        printf '%s/share/applications\n' "$prefix"
      fi
      ;;
    *)
      printf '%s/share/applications\n' "$(dirname "$dir")"
      ;;
  esac
}

launcher_name_ok() {
  case "$BIN_DST_NAME" in
    '' | *[[:cntrl:]]* | *'/'* | *'\\'* ) return 1 ;;
  esac
  return 0
}

desktop_path() {
  launcher_name_ok || {
    printf 'Error: BIN_NAME cannot be a desktop file name: %s\n' "$BIN_DST_NAME" >&2
    return 1
  }
  printf '%s/%s.desktop\n' "$(apps_dir)" "$BIN_DST_NAME"
}

# Quote one Exec argument. A literal % is %%.
desktop_quote() {
  local s=$1
  [[ "$s" != *$'\n'* ]] || return 1
  s=${s//\\/\\\\}
  s=${s//\"/\\\"}
  s=${s//\`/\\\`}
  s=${s//\$/\\\$}
  s=${s//%/%%}
  printf '"%s"' "$s"
}

write_desktop() {
  local dest=$1
  local bin=$2
  local exec_q
  exec_q="$(desktop_quote "$bin")" || fail "desktop path contains a newline: ${bin}"
  {
    printf '%s\n' "[Desktop Entry]"
    printf '%s\n' "Type=Application"
    printf '%s\n' "Version=1.0"
    printf '%s\n' "Name=${BIN_DST_NAME}"
    printf '%s\n' "GenericName=FTP client"
    printf '%s\n' "Comment=Terminal FTP, SFTP, and FTPS client"
    printf '%s\n' "Exec=${exec_q}"
    printf '%s\n' "TryExec=${bin}"
    printf '%s\n' "Terminal=true"
    printf '%s\n' "Categories=Network;FileTransfer;"
    printf '%s\n' "Keywords=ftp;sftp;ftps;file;transfer;"
    printf '%s\n' "StartupNotify=false"
    printf '%s\n' "Icon=utilities-terminal"
  } >"$dest"
  chmod 644 "$dest"
}

write_gui_desktop() {
  local dest=$1
  local bin=$2
  local exec_q
  exec_q="$(desktop_quote "$bin")" || fail "desktop path contains a newline: ${bin}"
  {
    printf '%s\n' "[Desktop Entry]"
    printf '%s\n' "Type=Application"
    printf '%s\n' "Version=1.0"
    printf '%s\n' "Name=dd_ftp"
    printf '%s\n' "GenericName=FTP client"
    printf '%s\n' "Comment=FTP, SFTP, and FTPS in a window"
    printf '%s\n' "Exec=${exec_q}"
    printf '%s\n' "TryExec=${bin}"
    printf '%s\n' "Terminal=false"
    printf '%s\n' "Categories=Network;FileTransfer;"
    printf '%s\n' "Keywords=ftp;sftp;ftps;file;transfer;"
    printf '%s\n' "StartupNotify=true"
    printf '%s\n' "StartupWMClass=${GUI_APP_ID}"
    printf '%s\n' "Icon=utilities-terminal"
  } >"$dest"
  chmod 644 "$dest"
}

refresh_desktop_database() {
  local dir=$1
  if have update-desktop-database; then
    update-desktop-database "$dir" >/dev/null 2>&1 || true
  fi
}

install_desktop_launcher() {
  local bin dest dir
  if [[ "$(detect_os)" != "linux" ]]; then
    info "desktop launcher skipped on $(detect_os)"
    return 0
  fi
  bin="$(installed_bin_path)"
  [[ -e "$bin" || -L "$bin" ]] || fail "cannot write a desktop launcher; binary missing at ${bin}"
  dest="$(desktop_path)" || fail "BIN_NAME cannot be a desktop file name: ${BIN_DST_NAME}"
  dir="$(dirname "$dest")"
  mkdir -p "$dir"
  write_desktop "$dest" "$bin"
  info "installed launcher ${dest}"
  refresh_desktop_database "$dir"
}

install_gui_launcher() {
  local bin dest dir
  if [[ "$(detect_os)" != "linux" ]]; then
    return 0
  fi
  bin="$(installed_gui_path)"
  [[ -e "$bin" || -L "$bin" ]] || return 0
  dest="$(gui_desktop_path)"
  dir="$(dirname "$dest")"
  mkdir -p "$dir"
  write_gui_desktop "$dest" "$bin"
  info "installed launcher ${dest}"
  refresh_desktop_database "$dir"
}

# Rewrite a launcher only for a binary that is still in INSTALL_DIR.
refresh_launchers() {
  if [[ "$(detect_os)" != "linux" ]]; then
    return 0
  fi
  local bin dest dir wrote=0
  bin="$(installed_bin_path)"
  if [[ -e "$bin" || -L "$bin" ]]; then
    if dest="$(desktop_path)"; then
      dir="$(dirname "$dest")"
      mkdir -p "$dir"
      write_desktop "$dest" "$bin"
      refresh_desktop_database "$dir"
      wrote=1
    fi
  fi
  bin="$(installed_gui_path)"
  if [[ -e "$bin" || -L "$bin" ]]; then
    dest="$(gui_desktop_path)"
    dir="$(dirname "$dest")"
    mkdir -p "$dir"
    write_gui_desktop "$dest" "$bin"
    refresh_desktop_database "$dir"
    wrote=1
  fi
  if [[ "$wrote" -eq 0 ]]; then
    refresh_desktop_database "$(apps_dir)"
  fi
}

remove_if_present() {
  local path=$1
  local label=$2
  if [[ -e "$path" || -L "$path" ]]; then
    rm -f -- "$path"
    info "removed ${path}"
  else
    info "${label} not found at ${path}"
  fi
}

remove_tui_files() {
  local bin desktop desktop_dir
  bin="$(installed_bin_path)"
  remove_if_present "$bin" "binary"
  if [[ "$(detect_os)" != "linux" ]]; then
    return 0
  fi
  if desktop="$(desktop_path)"; then
    desktop_dir="$(dirname "$desktop")"
    remove_if_present "$desktop" "launcher"
    refresh_desktop_database "$desktop_dir"
  else
    warn "skipped launcher removal; pass the same BIN_NAME used at install"
  fi
}

remove_gui_files() {
  local bin desktop
  bin="$(installed_gui_path)"
  remove_if_present "$bin" "window binary"
  if [[ "$(detect_os)" != "linux" ]]; then
    return 0
  fi
  desktop="$(gui_desktop_path)"
  remove_if_present "$desktop" "window launcher"
  refresh_desktop_database "$(dirname "$desktop")"
}

# $1 is tui, gui, or both. Only that binary and its .desktop file are removed.
do_uninstall() {
  local choice=$1
  local theme="${CONFIG_DIR}/${THEME_FILE}"
  case "$choice" in
    tui) remove_tui_files ;;
    gui) remove_gui_files ;;
    both)
      remove_tui_files
      remove_gui_files
      ;;
    *) fail "internal: unknown uninstall target '${choice}'" ;;
  esac

  refresh_launchers
  info "left ${theme}, bookmarks, and keyring secrets in place"

  if [[ -d "$CONFIG_DIR" ]] && rmdir "$CONFIG_DIR" 2>/dev/null; then
    info "removed empty ${CONFIG_DIR}"
  fi

  info "uninstall complete"
}

extract_package() {
  local archive="$1"
  local dest="$2"
  reject_unsafe_archive_members "$archive" || return 1
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

find_extracted_gui() {
  local dir="$1"
  local os="$2"
  local candidate
  if [[ "$os" == "windows" ]]; then
    for candidate in \
      "${dir}/dd_ftp-gui.exe" \
      "${dir}/bin/dd_ftp-gui.exe"
    do
      if [[ -f "$candidate" ]]; then
        printf '%s' "$candidate"
        return 0
      fi
    done
  else
    for candidate in \
      "${dir}/dd_ftp-gui" \
      "${dir}/bin/dd_ftp-gui"
    do
      if [[ -f "$candidate" ]]; then
        printf '%s' "$candidate"
        return 0
      fi
    done
  fi
  return 1
}

install_tui_from_release() {
  local triple version url archive tmp sums_url extracted src_bin dest theme
  triple="$(detect_target)"
  version="$(normalize_version "$VERSION")"
  url="${DD_FTP_PACKAGE_URL:-$(package_url "$triple" "$version")}"
  dest="$(installed_bin_path)"

  info "detected target ${triple}"
  info "downloading ${url}"

  tmp="$(mktemp -d)"
  CLEANUP_PATHS+=("$tmp")

  archive="${tmp}/dd_ftp-${triple}.tar.gz"
  if ! download "$url" "$archive" 2>/dev/null; then
    return 1
  fi

  sums_url="$(checksums_url "$version")"
  # Checksums are published with every release. A missing file is a failed
  # download, not a reason to unpack an unverified archive. DD_FTP_PACKAGE_URL
  # can replace the archive URL; the sums still come from REPO.
  download_optional "$sums_url" "${tmp}/SHA256SUMS" \
    || fail "could not download SHA256SUMS from ${sums_url}; refusing to install an unverified package"
  verify_checksum "$archive" "${tmp}/SHA256SUMS"

  extracted="${tmp}/pkg"
  extract_package "$archive" "$extracted" \
    || fail "refusing to extract $(basename "$archive")"
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

# GNU archive, distinct from the terminal package. A missing archive is not
# the terminal tarball installed under another name.
install_gui_from_release() {
  local triple version url archive tmp sums_url extracted src_bin dest
  triple="$(detect_target)"
  version="$(normalize_version "$VERSION")"
  url="${DD_FTP_GUI_PACKAGE_URL:-$(gui_package_url "$triple" "$version")}"
  dest="$(installed_gui_path)"

  info "downloading ${url}"
  tmp="$(mktemp -d)"
  CLEANUP_PATHS+=("$tmp")
  archive="${tmp}/dd_ftp-gui-${triple}.tar.gz"
  if ! download "$url" "$archive" 2>/dev/null; then
    return 1
  fi
  sums_url="$(checksums_url "$version")"
  download_optional "$sums_url" "${tmp}/SHA256SUMS" \
    || fail "could not download SHA256SUMS from ${sums_url}; refusing to install an unverified package"
  verify_checksum "$archive" "${tmp}/SHA256SUMS"
  extracted="${tmp}/pkg"
  extract_package "$archive" "$extracted" \
    || fail "refusing to extract $(basename "$archive")"
  src_bin="$(find_extracted_gui "$extracted" "$(detect_os)")" \
    || fail "package did not contain a dd_ftp-gui binary"
  install_file "$src_bin" "$dest" 0755
  info "installed ${dest}"
  info "dd_ftp-gui needs Vulkan (libvulkan1 and a driver) and Wayland or X11."
  return 0
}

build_and_install_from() {
  local src_dir="$1"
  local choice="$2"
  local dest src_bin

  have cargo || fail "cargo is required to build from source. Install Rust from https://rustup.rs/"

  if [[ "$choice" == "tui" || "$choice" == "both" ]]; then
    dest="$(installed_bin_path)"
    info "building dd_ftp (release) from ${src_dir}"
    # Not --workspace: dd_ftp_gui is a member but not a default member.
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
  fi

  if [[ "$choice" == "gui" || "$choice" == "both" ]]; then
    dest="$(installed_gui_path)"
    info "building dd_ftp-gui (release) from ${src_dir}"
    cargo build --release -p dd_ftp_gui --manifest-path "${src_dir}/Cargo.toml"
    if [[ "$(detect_os)" == "windows" ]]; then
      src_bin="${src_dir}/target/release/dd_ftp-gui.exe"
    else
      src_bin="${src_dir}/target/release/dd_ftp-gui"
    fi
    [[ -f "$src_bin" ]] || fail "built binary not found at ${src_bin}"
    install_file "$src_bin" "$dest" 0755
    info "installed ${dest}"
    info "dd_ftp-gui needs Vulkan (libvulkan1 and a driver) and Wayland or X11."
  fi
}

install_from_source() {
  local choice="$1"
  local src_dir workdir clone_args
  if is_local_checkout; then
    src_dir="$(script_dir)"
    build_and_install_from "$src_dir" "$choice"
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
  build_and_install_from "${workdir}/src" "$choice"
}

run_self_test() {
  local work hash name bad
  have python3 || fail "self-test requires python3"
  have tar || fail "self-test requires tar"
  file_sha256 /dev/null >/dev/null || fail "self-test requires sha256sum or shasum"

  [[ "$(choose_target install tui)" == "tui" ]] || fail "self-test: explicit tui choice"
  [[ "$(choose_target install gui)" == "gui" ]] || fail "self-test: explicit gui choice"
  [[ "$(choose_target uninstall both)" == "both" ]] || fail "self-test: explicit both choice"

  work="$(mktemp -d)"
  CLEANUP_PATHS+=("$work")

  python3 - "$work" <<'PY'
import io, os, sys, tarfile

work = sys.argv[1]

def write(path, members):
    with tarfile.open(path, "w:gz") as tar:
        for name, data in members:
            info = tarfile.TarInfo(name)
            raw = data.encode()
            info.size = len(raw)
            info.mode = 0o644
            tar.addfile(info, io.BytesIO(raw))

write(os.path.join(work, "traversal.tar.gz"), [("../escaped", "nope")])
write(os.path.join(work, "absolute.tar.gz"), [("/tmp/dd_ftp_should_not_exist", "nope")])
write(os.path.join(work, "nested.tar.gz"), [("pkg/../../escaped", "nope")])
write(
    os.path.join(work, "good.tar.gz"),
    [
        ("dd_ftp", "#!/bin/sh\n"),
        ("dd_ftp_theme.yml", "ok\n"),
        ("foo..bar", "ok\n"),
    ],
)
PY

  for bad in traversal absolute nested; do
    if reject_unsafe_archive_members "${work}/${bad}.tar.gz" 2>/dev/null; then
      fail "self-test: ${bad} archive was accepted"
    fi
  done

  mkdir -p "${work}/dest"
  if extract_package "${work}/traversal.tar.gz" "${work}/dest" 2>/dev/null; then
    fail "self-test: traversal archive was extracted"
  fi
  [[ ! -e "${work}/escaped" && ! -e "${work}/dest/escaped" ]] \
    || fail "self-test: traversal member was written"
  [[ ! -e /tmp/dd_ftp_should_not_exist ]] \
    || fail "self-test: absolute member was written"

  name="dd_ftp-self-test.tar.gz"
  cp "${work}/good.tar.gz" "${work}/${name}"
  hash="$(file_sha256 "${work}/${name}")"
  printf '%s  %s\n' "$hash" "$name" >"${work}/SHA256SUMS"
  verify_checksum "${work}/${name}" "${work}/SHA256SUMS"
  printf '%s *%s\n' "$hash" "$name" >"${work}/SHA256SUMS"
  verify_checksum "${work}/${name}" "${work}/SHA256SUMS"
  extract_package "${work}/${name}" "${work}/good-out"
  [[ -f "${work}/good-out/dd_ftp" ]] || fail "self-test: dd_ftp was not extracted"
  [[ -f "${work}/good-out/foo..bar" ]] || fail "self-test: a name containing '..' as text was rejected"
  [[ -f "${work}/good-out/dd_ftp_theme.yml" ]] || fail "self-test: theme was not extracted"

  printf '0000000000000000000000000000000000000000000000000000000000000000  %s\n' "$name" \
    >"${work}/badsums"
  if ( trap - EXIT; verify_checksum "${work}/${name}" "${work}/badsums" ) 2>/dev/null; then
    fail "self-test: checksum mismatch was accepted"
  fi

  printf 'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa  other.tar.gz\n' \
    >"${work}/nosuch"
  if ( trap - EXIT; verify_checksum "${work}/${name}" "${work}/nosuch" ) 2>/dev/null; then
    fail "self-test: archive with no checksum entry was accepted"
  fi

  # Launcher paths stay inside the temp tree. These overrides end with the
  # process: --self-test exits as soon as this function returns.
  HOME="${work}/home"
  INSTALL_DIR="${HOME}/.local/bin"
  XDG_DATA_HOME=""
  CONFIG_DIR="${work}/config"
  BIN_DST_NAME="dd_ftp"
  mkdir -p "$INSTALL_DIR" "$CONFIG_DIR"
  printf '#!/bin/sh\nexit 0\n' >"$(installed_bin_path)"
  chmod 755 "$(installed_bin_path)"
  printf 'theme\n' >"${CONFIG_DIR}/${THEME_FILE}"

  [[ "$(apps_dir)" == "${HOME}/.local/share/applications" ]] \
    || fail "self-test: user apps dir was $(apps_dir)"

  XDG_DATA_HOME="${work}/xdg"
  [[ "$(apps_dir)" == "${XDG_DATA_HOME}/applications" ]] \
    || fail "self-test: XDG apps dir was $(apps_dir)"

  INSTALL_DIR="/usr/local/bin"
  [[ "$(apps_dir)" == "/usr/local/share/applications" ]] \
    || fail "self-test: /usr/local apps dir was $(apps_dir)"

  INSTALL_DIR="/usr/bin"
  [[ "$(apps_dir)" == "/usr/share/applications" ]] \
    || fail "self-test: /usr apps dir was $(apps_dir)"

  INSTALL_DIR="/bin"
  [[ "$(apps_dir)" == "/share/applications" ]] \
    || fail "self-test: /bin apps dir was $(apps_dir)"

  INSTALL_DIR="/opt/dd_ftp"
  [[ "$(apps_dir)" == "/opt/share/applications" ]] \
    || fail "self-test: non-bin prefix apps dir was $(apps_dir)"

  INSTALL_DIR="${HOME}/.local/bin"
  XDG_DATA_HOME="${work}/xdg"
  install_desktop_launcher
  local desk="${XDG_DATA_HOME}/applications/dd_ftp.desktop"
  local bin_path
  bin_path="$(installed_bin_path)"
  [[ -f "$desk" ]] || fail "self-test: launcher was not written"
  [[ "$(stat -c '%a' "$desk")" == "644" ]] || fail "self-test: launcher mode was $(stat -c '%a' "$desk")"
  grep -F 'Terminal=true' "$desk" >/dev/null || fail "self-test: launcher is not a terminal app"
  grep -F 'Categories=Network;FileTransfer;' "$desk" >/dev/null \
    || fail "self-test: launcher categories missing"
  grep -F "Exec=$(desktop_quote "$bin_path")" "$desk" >/dev/null \
    || fail "self-test: launcher Exec does not match ${bin_path}"
  grep -F "TryExec=${bin_path}" "$desk" >/dev/null \
    || fail "self-test: launcher TryExec does not match ${bin_path}"
  if have desktop-file-validate; then
    desktop-file-validate "$desk" || fail "self-test: desktop-file-validate failed"
  fi

  # A fake window binary is enough. This test does not build GPUI.
  local gui_path gui_desk
  gui_path="$(installed_gui_path)"
  printf '#!/bin/sh\nexit 0\n' >"$gui_path"
  chmod 755 "$gui_path"
  install_gui_launcher
  gui_desk="$(gui_desktop_path)"
  [[ -f "$gui_desk" ]] || fail "self-test: window launcher was not written"
  grep -F 'Terminal=false' "$gui_desk" >/dev/null \
    || fail "self-test: window launcher is a terminal app"
  grep -F 'StartupNotify=true' "$gui_desk" >/dev/null \
    || fail "self-test: window launcher StartupNotify"
  grep -F "StartupWMClass=${GUI_APP_ID}" "$gui_desk" >/dev/null \
    || fail "self-test: window launcher StartupWMClass"
  grep -F "Exec=$(desktop_quote "$gui_path")" "$gui_desk" >/dev/null \
    || fail "self-test: window Exec does not match ${gui_path}"
  if have desktop-file-validate; then
    desktop-file-validate "$gui_desk" || fail "self-test: window desktop-file-validate failed"
  fi

  do_uninstall gui
  [[ ! -e "$gui_desk" ]] || fail "self-test: window launcher survived uninstall"
  [[ ! -e "$gui_path" ]] || fail "self-test: window binary survived uninstall"
  [[ -e "$bin_path" ]] || fail "self-test: terminal binary was removed with the window"
  [[ -e "$desk" ]] || fail "self-test: terminal launcher was removed with the window"
  [[ -e "${CONFIG_DIR}/${THEME_FILE}" ]] \
    || fail "self-test: theme was removed with the window"

  do_uninstall tui
  [[ ! -e "$desk" ]] || fail "self-test: launcher survived uninstall"
  [[ ! -e "$bin_path" ]] || fail "self-test: binary survived uninstall"
  [[ -e "${CONFIG_DIR}/${THEME_FILE}" ]] \
    || fail "self-test: theme was removed"

  local quoted
  quoted="$(desktop_quote '/tmp/a b%c$"')" || fail "self-test: quote rejected a normal path"
  [[ "$quoted" == '"/tmp/a b%%c\$\""' ]] || fail "self-test: quote produced ${quoted}"
  if desktop_quote $'/tmp/has\nnewline' >/dev/null 2>&1; then
    fail "self-test: a newline in the binary path was accepted"
  fi

  BIN_DST_NAME="dd/ftp"
  if desktop_path >/dev/null 2>&1; then
    fail "self-test: a slash in BIN_NAME was accepted"
  fi

  BIN_DST_NAME="dd ftp"
  HOME="${work}/spaced-home"
  INSTALL_DIR="${HOME}/.local/bin"
  XDG_DATA_HOME="${work}/spaced-xdg"
  mkdir -p "$INSTALL_DIR"
  printf '#!/bin/sh\nexit 0\n' >"${INSTALL_DIR}/dd ftp"
  chmod 755 "${INSTALL_DIR}/dd ftp"
  install_desktop_launcher
  local spaced_desk
  spaced_desk="$(desktop_path)"
  [[ "$spaced_desk" == "${XDG_DATA_HOME}/applications/dd ftp.desktop" ]] \
    || fail "self-test: spaced launcher path was ${spaced_desk}"
  [[ -f "$spaced_desk" ]] || fail "self-test: spaced launcher name was not written"
  grep -F "Exec=$(desktop_quote "${INSTALL_DIR}/dd ftp")" "$spaced_desk" >/dev/null \
    || fail "self-test: spaced Exec was not quoted"

  do_uninstall tui
  [[ ! -e "$spaced_desk" ]] || fail "self-test: spaced launcher survived uninstall"
  [[ ! -e "${INSTALL_DIR}/dd ftp" ]] \
    || fail "self-test: spaced binary survived uninstall"

  info "self-test ok"
}

install_programs() {
  local choice=$1
  local prefer_source=0
  if [[ "$FROM_SOURCE" -eq 1 ]]; then
    prefer_source=1
  elif [[ "$FROM_RELEASE" -eq 1 ]]; then
    prefer_source=0
  elif is_local_checkout && have cargo; then
    prefer_source=1
  fi

  if [[ "$prefer_source" -eq 1 ]]; then
    install_from_source "$choice"
    return 0
  fi

  if [[ "$choice" == "tui" || "$choice" == "both" ]]; then
    if install_tui_from_release; then
      :
    elif [[ "$FROM_RELEASE" -eq 1 ]]; then
      fail "no prebuilt package for $(detect_target). Check https://github.com/${REPO}/releases"
    else
      warn "no prebuilt package for $(detect_target); building from source"
      install_from_source tui
    fi
  fi

  if [[ "$choice" == "gui" || "$choice" == "both" ]]; then
    if install_gui_from_release; then
      :
    elif [[ "$FROM_RELEASE" -eq 1 ]]; then
      fail "no published dd_ftp-gui archive for $(detect_target). The window is built from source: ./install.sh --from-source --gui"
    else
      warn "no published dd_ftp-gui archive for $(detect_target); building the window from source"
      install_from_source gui
    fi
  fi
}

main() {
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --tui | --gui | --both)
        if [[ -n "$TARGET_CHOICE" ]]; then
          fail "pass only one of --tui, --gui, or --both"
        fi
        TARGET_CHOICE="${1#--}"
        shift
        ;;
      --from-release) FROM_RELEASE=1; shift ;;
      --from-source) FROM_SOURCE=1; shift ;;
      --uninstall | -uninstall) UNINSTALL=1; shift ;;
      --print-target) PRINT_TARGET=1; shift ;;
      --dry-run) DRY_RUN=1; shift ;;
      --self-test)
        run_self_test
        exit 0
        ;;
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

  if [[ -z "$TARGET_CHOICE" && "${INSTALL_GUI:-}" == "1" ]]; then
    TARGET_CHOICE=both
  fi

  if [[ "$PRINT_TARGET" -eq 1 ]]; then
    detect_target
    printf '\n'
    exit 0
  fi

  if [[ "$FROM_RELEASE" -eq 1 && "$FROM_SOURCE" -eq 1 ]]; then
    fail "use either --from-release or --from-source, not both"
  fi

  print_mark

  if [[ "$DRY_RUN" -eq 1 ]]; then
    printf 'programs:    %s\n' "${TARGET_CHOICE:-ask}"
    printf 'target:      %s\n' "$(detect_target)"
    printf 'package:     %s\n' "$(package_url "$(detect_target)" "$(normalize_version "$VERSION")")"
    printf 'window pkg:  %s\n' "$(gui_package_url "$(detect_target)" "$(normalize_version "$VERSION")")"
    printf 'install to:  %s\n' "$(installed_bin_path)"
    printf 'window bin:  %s\n' "$(installed_gui_path)"
    printf 'theme:       %s/%s\n' "$CONFIG_DIR" "$THEME_FILE"
    if [[ "$(detect_os)" == "linux" ]]; then
      printf 'launcher:    %s\n' "$(desktop_path)"
      printf 'window ln:   %s\n' "$(gui_desktop_path)"
    else
      printf 'launcher:    desktop files are Linux-only\n'
    fi
    if is_local_checkout; then
      printf 'checkout:    %s\n' "$(script_dir)"
    else
      printf 'checkout:    (none; curl/piped install)\n'
    fi
    exit 0
  fi

  local action=install choice
  if [[ "$UNINSTALL" -eq 1 ]]; then
    action=uninstall
  fi
  choice="$(choose_target "$action" "$TARGET_CHOICE")"

  if [[ "$action" == uninstall ]]; then
    do_uninstall "$choice"
    exit 0
  fi

  install_programs "$choice"
  refresh_launchers
  path_note
  case "$choice" in
    tui)
      printf '\nRun: %s\n' "$BIN_DST_NAME"
      ;;
    gui)
      printf '\nRun: %s\n' "$GUI_BIN_NAME"
      printf '%s\n' "dd_ftp-gui needs Vulkan (libvulkan1 and a driver) and Wayland or X11."
      ;;
    both)
      printf '\nRun: %s    or    %s\n' "$BIN_DST_NAME" "$GUI_BIN_NAME"
      printf '%s\n' "dd_ftp-gui needs Vulkan (libvulkan1 and a driver) and Wayland or X11."
      ;;
  esac
  if [[ "$(detect_os)" == "linux" ]]; then
    if [[ "$choice" != "gui" ]]; then
      printf 'Launcher: %s\n' "$(desktop_path)"
    fi
    if [[ "$choice" != "tui" ]]; then
      printf 'Launcher: %s\n' "$(gui_desktop_path)"
    fi
  fi
}

# `curl | bash` reads this file from stdin. BASH_SOURCE is unset then, and
# `set -u` aborts on a bare ${BASH_SOURCE[0]}. An empty source means the
# file is being executed, not sourced.
if [[ -z "${BASH_SOURCE[0]:-}" || "${BASH_SOURCE[0]:-}" == "$0" ]]; then
  main "$@"
fi
