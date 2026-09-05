#!/bin/sh
#
# On Air Record installer.
#
#   curl -fsSL https://raw.githubusercontent.com/shibbirweb/on-air-record/master/scripts/install.sh | sh
#
# Everything lands in an `on-air-record` folder created in whatever directory you run this from: the
# program, your settings, and the recordings. Nothing is written anywhere else on the machine, so moving
# or removing the whole installation is moving or removing that one folder.
#
# It works out which build this machine needs, downloads it from the latest release, checks it against the
# published checksum, asks once which port to use, remembers the answer, and starts the service.
#
# POSIX sh on purpose: this has to run under dash on Debian as happily as under bash on a Mac.

set -eu

REPOSITORY="shibbirweb/on-air-record"
DEFAULT_PORT=8080
FOLDER_NAME="on-air-record"

install_dir=""
want_reconfigure=0
want_update=0
want_start=1
forced_port=""
forced_version=

say()  { printf '%s\n' "$*"; }
step() { printf '\n==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'USAGE'
Usage: install.sh [options]

Creates an `on-air-record` folder in the current directory and installs into it.

  --dir <path>      Install into this folder instead
  --port <number>   Use this port and do not ask
  --release <tag>   Install this exact version, like v0.1.0, instead of the latest
  --reconfigure     Ask for the port again, even if a config already exists
  --update          Download the latest release even if a program is already installed
  --no-start        Install and configure, but do not start the service
  --help            Show this message

What the folder holds:

  on-air-record/on-air-record   the program
  on-air-record/start.sh        starts it with your settings
  on-air-record/config          your settings
  on-air-record/data            recordings and the database
USAGE
}

while [ $# -gt 0 ]; do
  case "$1" in
    --dir) [ $# -ge 2 ] || die "--dir needs a path"; install_dir="$2"; shift 2 ;;
    --dir=*) install_dir="${1#--dir=}"; shift ;;
    --port) [ $# -ge 2 ] || die "--port needs a number"; forced_port="$2"; shift 2 ;;
    --port=*) forced_port="${1#--port=}"; shift ;;
    --release) [ $# -ge 2 ] || die "--release needs a version, like v0.1.0"; forced_version="$2"; shift 2 ;;
    --release=*) forced_version="${1#--release=}"; shift ;;
    --reconfigure) want_reconfigure=1; shift ;;
    --update) want_update=1; shift ;;
    --no-start) want_start=0; shift ;;
    --help|-h) usage; exit 0 ;;
    *) die "unknown option: $1. Try --help." ;;
  esac
done

[ -n "$install_dir" ] || install_dir="$PWD/$FOLDER_NAME"

BINARY="$install_dir/on-air-record"
CONFIG_FILE="$install_dir/config"
LAUNCHER="$install_dir/start.sh"
STOPPER="$install_dir/stop.sh"
DATA_DIR="$install_dir/data"

need() { command -v "$1" >/dev/null 2>&1 || die "this script needs $1, which is not installed"; }

# ---------------------------------------------------------------- which build

detect_target() {
  os="$(uname -s)"
  arch="$(uname -m)"

  case "$os" in
    Darwin)
      case "$arch" in
        arm64|aarch64) printf 'aarch64-apple-darwin' ;;
        x86_64) printf 'x86_64-apple-darwin' ;;
        *) die "unsupported Mac architecture: $arch" ;;
      esac
      ;;
    Linux)
      case "$arch" in
        x86_64|amd64) printf 'x86_64-unknown-linux-gnu' ;;
        aarch64|arm64|armv7l|armv6l)
          die "there is no prebuilt binary for Linux on $arch yet, so this needs building from source.
See https://github.com/$REPOSITORY/blob/master/docs/SETUP.md#building-from-source" ;;
        *) die "unsupported Linux architecture: $arch" ;;
      esac
      ;;
    MINGW*|MSYS*|CYGWIN*)
      die "Windows is not installed by this script. Download the .zip from
https://github.com/$REPOSITORY/releases/latest and see docs/SETUP.md for running it as a service." ;;
    *)
      die "unsupported operating system: $os" ;;
  esac
}

# The redirect on /releases/latest names the tag, which keeps this off the API and its hourly rate limit.
latest_version() {
  url="$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$REPOSITORY/releases/latest")" \
    || die "could not reach GitHub to find the latest release"
  version="${url##*/}"
  case "$version" in
    v*) printf '%s' "$version" ;;
    *) die "could not work out the latest version from $url" ;;
  esac
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | cut -d' ' -f1
  else
    printf ''
  fi
}

install_release() {
  target="$1"
  version="$2"
  name="on-air-record-$version-$target"
  base="https://github.com/$REPOSITORY/releases/download/$version"

  tmp="$(mktemp -d)"
  # shellcheck disable=SC2064
  trap "rm -rf '$tmp'" EXIT INT TERM

  say "  downloading $name.tar.gz"
  curl -fL --progress-bar -o "$tmp/archive.tar.gz" "$base/$name.tar.gz" \
    || die "could not download $base/$name.tar.gz"

  if curl -fsSL -o "$tmp/archive.sha256" "$base/$name.tar.gz.sha256" 2>/dev/null; then
    expected="$(cut -d' ' -f1 < "$tmp/archive.sha256")"
    actual="$(sha256_of "$tmp/archive.tar.gz")"
    if [ -z "$actual" ]; then
      warn "no sha256sum or shasum on this machine, so the download was not verified"
    elif [ "$expected" != "$actual" ]; then
      die "the download is corrupt: expected $expected, got $actual"
    else
      say "  checksum verified"
    fi
  else
    warn "no published checksum for this build, so the download was not verified"
  fi

  tar -xzf "$tmp/archive.tar.gz" -C "$tmp" || die "could not unpack the download"
  [ -f "$tmp/$name/on-air-record" ] || die "the archive did not contain the program"

  cp "$tmp/$name/on-air-record" "$BINARY"
  chmod 755 "$BINARY"

  for extra in README.md LICENSE on-air-record.service; do
    if [ -f "$tmp/$name/$extra" ]; then
      cp "$tmp/$name/$extra" "$install_dir/"
    fi
  done

  rm -rf "$tmp"
  trap - EXIT INT TERM
}

# ---------------------------------------------------------------- asking

# When this script is piped into sh, stdin is the script itself, so questions have to be read from the
# terminal directly. Without a terminal at all there is nothing to ask, and the defaults stand.
ask() {
  prompt="$1"
  fallback="$2"
  answer=""

  if [ -t 0 ]; then
    printf '%s' "$prompt" >&2
    IFS= read -r answer || answer=""
  elif (exec < /dev/tty) 2>/dev/null; then
    # Readable by permission is not the same as openable: with no controlling terminal, as under cron or
    # a CI runner, /dev/tty exists and still fails to open. Try it for real before promising a prompt.
    #
    # The test has to happen inside a subshell. In dash, which is /bin/sh on Debian and Ubuntu, a failed
    # redirection on a compound command kills the shell outright with status 2, and 2>/dev/null hides the
    # message without preventing the death. A subshell contains it, and `if` just sees a false condition.
    # bash is more forgiving, which is exactly why this passed on macOS and died on Ubuntu.
    printf '%s' "$prompt" >&2
    IFS= read -r answer < /dev/tty || answer=""
  else
    printf '%s' "$fallback"
    return 0
  fi

  [ -n "$answer" ] || answer="$fallback"
  printf '%s' "$answer"
}

port_is_free() {
  if command -v lsof >/dev/null 2>&1; then
    ! lsof -nP -iTCP:"$1" -sTCP:LISTEN >/dev/null 2>&1
  elif command -v ss >/dev/null 2>&1; then
    ! ss -lnt 2>/dev/null | grep -q ":$1 "
  else
    return 0
  fi
}

choose_port() {
  while :; do
    port="$(ask "Which port should the web interface use? [$DEFAULT_PORT] " "$DEFAULT_PORT")"

    case "$port" in
      ''|*[!0-9]*) say "  '$port' is not a number, try again." >&2; continue ;;
    esac
    if [ "$port" -lt 1024 ] || [ "$port" -gt 65535 ]; then
      say "  pick a number between 1024 and 65535." >&2
      continue
    fi
    if ! port_is_free "$port"; then
      say "  something is already listening on $port, pick another." >&2
      continue
    fi

    printf '%s' "$port"
    return 0
  done
}

write_config() {
  cat > "$CONFIG_FILE" <<CONFIG
# On Air Record settings, read every time start.sh runs.
# Edit by hand, or re-run the installer with --reconfigure.
# Everything else (microphone, retention, quality) is set in the web interface itself.

OAR_PORT=$1
OAR_HOST=0.0.0.0
OAR_LOG_LEVEL=info
CONFIG
}

# The launcher resolves its own folder at run time rather than baking in an absolute path, so the whole
# installation can be moved or copied to another machine and still work.
write_launcher() {
  cat > "$LAUNCHER" <<'LAUNCHER_SCRIPT'
#!/bin/sh
# Starts On Air Record with the settings in the config file beside this script.
# Written by the installer. Arguments are passed through, so a flag beats the config file for one run.
set -eu

here="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"

if [ -f "$here/config" ]; then
  . "$here/config"
  export OAR_PORT OAR_HOST OAR_LOG_LEVEL
fi

# Recordings always live beside the program, so the folder stays self contained.
OAR_DATA_DIR="$here/data"
export OAR_DATA_DIR

exec "$here/on-air-record" "$@"
LAUNCHER_SCRIPT
  chmod 755 "$LAUNCHER"

  cat > "$STOPPER" <<'STOP_SCRIPT'
#!/bin/sh
# Stops the On Air Record started from this folder.
# Written by the installer.
#
# It matches on this installation's own program path rather than on the name, so a second copy running
# from somewhere else is left alone. And it sends TERM rather than KILL, because the service closes and
# indexes the segment it is part way through writing on the way out. KILL loses those seconds.
set -eu

here="$(CDPATH='' cd -- "$(dirname -- "$0")" && pwd)"
program="$here/on-air-record"

# pgrep lives in procps, which a minimal install may not have. The fallback reads the same thing out of ps.
running() {
  if command -v pgrep > /dev/null 2>&1; then
    pgrep -f "$program" 2>/dev/null || true
  else
    ps -eo pid=,args= 2>/dev/null | awk -v p="$program" 'index($0, p) { print $1 }' || true
  fi
}

pids="$(running)"
if [ -z "$pids" ]; then
  echo "Nothing is running from $here."
  exit 0
fi

echo "Stopping $(echo "$pids" | tr '\n' ' ')"
for pid in $pids; do
  kill "$pid" 2>/dev/null || true
done

# Long enough for the open segment to be flushed and indexed, which is the whole reason for TERM.
waited=0
while [ "$waited" -lt 20 ]; do
  [ -z "$(running)" ] && { echo "Stopped."; exit 0; }
  sleep 1
  waited=$((waited + 1))
done

echo "It is still running after ${waited}s. Something is wedged, so force it:" >&2
echo "  kill -9 $(running | tr '\n' ' ')" >&2
echo "That loses the few seconds of audio not yet written out." >&2
exit 1
STOP_SCRIPT
  chmod 755 "$STOPPER"
}

# A path relative to where the user is standing reads better than an absolute one in the closing message.
relative_to_pwd() {
  case "$1" in
    "$PWD"/*) printf './%s' "${1#"$PWD"/}" ;;
    *) printf '%s' "$1" ;;
  esac
}

# ---------------------------------------------------------------- run

need curl
need tar
need uname

say "On Air Record installer"

target="$(detect_target)"
say "  this machine:  $(uname -s) $(uname -m)  ->  $target"
say "  installing to: $(relative_to_pwd "$install_dir")"

parent="$(dirname -- "$install_dir")"
[ -d "$parent" ] || die "$parent does not exist"
[ -w "$parent" ] || die "cannot write to $parent, so the folder cannot be created there"

mkdir -p "$install_dir" "$DATA_DIR"

if [ -n "$forced_version" ]; then
  # An explicit version is an instruction, not a preference, so it overwrites whatever is already here.
  case "$forced_version" in
    v*) ;;
    *) die "--release wants a tag like v0.1.0, not $forced_version" ;;
  esac
  step "Installing $forced_version"
  install_release "$target" "$forced_version"
  say "  installed"
elif [ -x "$BINARY" ] && [ "$want_update" -eq 0 ]; then
  installed="$("$BINARY" --version 2>/dev/null | awk '{print $2}')"
  step "Already installed here: version ${installed:-unknown}"
  say "  run with --update to fetch the latest release"
else
  version="$(latest_version)"
  step "Installing $version"
  install_release "$target" "$version"
  say "  installed"
fi

if [ -n "$forced_port" ]; then
  step "Configuring"
  write_config "$forced_port"
  say "  port $forced_port, from --port"
elif [ -f "$CONFIG_FILE" ] && [ "$want_reconfigure" -eq 0 ]; then
  # shellcheck disable=SC1090
  . "$CONFIG_FILE"
  step "Using the settings already saved in this folder"
  say "  port ${OAR_PORT:-$DEFAULT_PORT}"
  say "  re-run with --reconfigure to change them"
else
  step "First run, so one question"
  say "  The web interface needs a port. $DEFAULT_PORT is the default; press enter to accept it."
  say ""
  chosen="$(choose_port)"
  write_config "$chosen"
  say ""
  say "  saved"
fi

write_launcher

# shellcheck disable=SC1090
. "$CONFIG_FILE"
port="${OAR_PORT:-$DEFAULT_PORT}"

step "Ready"
say "  start it again:  $(relative_to_pwd "$LAUNCHER")"
say "  stop it:         $(relative_to_pwd "$STOPPER")"
say "  open:            http://localhost:$port"
say "  settings:        $(relative_to_pwd "$CONFIG_FILE")"
say "  recordings:      $(relative_to_pwd "$DATA_DIR")"
say ""
say "  To remove it completely, delete $(relative_to_pwd "$install_dir")."

if [ "$want_start" -eq 1 ]; then
  step "Starting on port $port"
  say "  Ctrl+C stops it while this terminal is open. Afterwards, or if the terminal goes away with the"
  say "  service still running, use $(relative_to_pwd "$STOPPER")."
  say ""
  exec "$LAUNCHER"
fi
