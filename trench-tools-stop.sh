#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<'EOF'
Usage: ./trench-tools-stop.sh [--mode ee|ld|both]
EOF
}

load_env_file() {
  # Hand-parse the .env file instead of `source`ing it so Windows-authored
  # files (CRLF line endings, possibly quoted values) do not corrupt variable
  # names with a trailing `$'\r'` and so quoted values get unwrapped cleanly.
  local env_path="$project_root/.env"
  [[ -f "$env_path" ]] || return 0
  local line stripped key value
  while IFS= read -r line || [[ -n "$line" ]]; do
    stripped="${line%$'\r'}"
    stripped="${stripped#"${stripped%%[![:space:]]*}"}"
    [[ -z "$stripped" || "$stripped" == "#"* ]] && continue
    if [[ "$stripped" == "export "* ]]; then
      stripped="${stripped#export }"
    fi
    [[ "$stripped" == *"="* ]] || continue
    key="${stripped%%=*}"
    value="${stripped#*=}"
    key="${key#"${key%%[![:space:]]*}"}"
    key="${key%"${key##*[![:space:]]}"}"
    [[ -z "$key" || ! "$key" =~ ^[A-Za-z_][A-Za-z0-9_]*$ ]] && continue
    if [[ ${#value} -ge 2 ]]; then
      local first="${value:0:1}"
      local last="${value: -1}"
      if [[ "$first" == '"' && "$last" == '"' ]] || [[ "$first" == "'" && "$last" == "'" ]]; then
        value="${value:1:${#value}-2}"
      fi
    fi
    export "$key=$value"
  done <"$env_path"
}

resolve_path() {
  local raw_path="$1"
  if [[ "$raw_path" = /* ]]; then
    printf '%s\n' "$raw_path"
  else
    printf '%s\n' "$project_root/$raw_path"
  fi
}

validate_mode() {
  local normalized
  normalized="$(printf '%s' "$1" | tr '[:upper:]' '[:lower:]')"
  case "$normalized" in
    ee|ld|both)
      printf '%s\n' "$normalized"
      ;;
    *)
      echo "Error: mode must be ee, ld, or both." >&2
      exit 1
      ;;
  esac
}

port_for_binary() {
  case "$1" in
    execution-engine) printf '%s\n' "$execution_engine_port" ;;
    launchdeck-engine) printf '%s\n' "$launchdeck_port" ;;
    launchdeck-follow-daemon) printf '%s\n' "$launchdeck_follow_daemon_port" ;;
    *) printf '%s\n' "" ;;
  esac
}

target_binaries_for_mode() {
  case "$1" in
    ee)
      printf '%s\n' "execution-engine"
      ;;
    ld)
      printf '%s\n' "launchdeck-engine" "launchdeck-follow-daemon"
      ;;
    both)
      printf '%s\n' "execution-engine" "launchdeck-engine" "launchdeck-follow-daemon"
      ;;
  esac
}

list_ancestor_pids() {
  local pid="$1"
  while [[ -n "$pid" && "$pid" =~ ^[0-9]+$ && "$pid" -gt 1 ]]; do
    printf '%s\n' "$pid"
    pid="$(ps -o ppid= -p "$pid" 2>/dev/null | awk '{print $1}')"
  done
}

is_excluded_pid() {
  local candidate="$1"
  local excluded
  while IFS= read -r excluded; do
    [[ -n "$excluded" ]] || continue
    if [[ "$candidate" == "$excluded" ]]; then
      return 0
    fi
  done < <(list_ancestor_pids "$$")
  return 1
}

list_listening_pids_for_port() {
  local port="$1"
  [[ -n "$port" ]] || return 0

  # Prefer lsof first: it's portable across Linux and macOS and avoids the
  # GNU-vs-mawk `match()` incompatibility that the `ss` fallback has.
  if command -v lsof >/dev/null 2>&1; then
    lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true
    return
  fi

  if command -v ss >/dev/null 2>&1; then
    # Extract pid=... with plain tools so this works under both gawk and mawk.
    ss -ltnp "( sport = :$port )" 2>/dev/null \
      | tr ',' '\n' \
      | sed -n 's/.*pid=\([0-9][0-9]*\).*/\1/p'
    return
  fi

  if command -v netstat >/dev/null 2>&1; then
    netstat -ltnp 2>/dev/null \
      | awk -v port=":$port" '$4 ~ port { split($7, parts, "/"); if (parts[1] ~ /^[0-9]+$/) print parts[1] }'
  fi
}

pid_matches_binary() {
  local pid="$1"
  local expected_binary="$2"

  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  [[ -n "$expected_binary" ]] || return 0

  local cmdline=""
  if [[ -r "/proc/$pid/comm" ]]; then
    cmdline="$(tr -d '\n' < "/proc/$pid/comm" 2>/dev/null || true)"
    if [[ -n "$cmdline" && "$cmdline" == "$expected_binary" ]]; then
      return 0
    fi
  fi
  if [[ -r "/proc/$pid/cmdline" ]]; then
    cmdline="$(tr '\0' ' ' < "/proc/$pid/cmdline" 2>/dev/null || true)"
  elif command -v ps >/dev/null 2>&1; then
    cmdline="$(ps -p "$pid" -o command= 2>/dev/null || true)"
  fi
  # Fail closed: if we cannot discover the command line at all, refuse to
  # match. A stale pidfile pointing at a recycled PID could otherwise cause
  # us to kill an unrelated process we weren't able to identify.
  if [[ -z "$cmdline" ]]; then
    return 1
  fi
  if [[ "$cmdline" == *"$expected_binary"* ]]; then
    return 0
  fi
  return 1
}

terminate_pid() {
  local pid="$1"
  local label="$2"
  local force="${3:-0}"
  local expected_binary="${4:-}"

  [[ "$pid" =~ ^[0-9]+$ ]] || return 1
  [[ "$pid" != "$$" ]] || return 1
  is_excluded_pid "$pid" && return 1
  if ! kill -0 "$pid" 2>/dev/null; then
    return 1
  fi
  if [[ -n "$expected_binary" ]] && ! pid_matches_binary "$pid" "$expected_binary"; then
    echo "Skipping PID $pid for $label: does not match expected binary '$expected_binary' (likely a recycled PID in a stale file)." >&2
    return 1
  fi

  if (( force )); then
    kill -KILL "$pid" 2>/dev/null || true
  else
    kill -TERM "$pid" 2>/dev/null || true
  fi
  printf '%s\n' "$pid"
  return 0
}

add_pending_stop() {
  local pid="$1"
  local label="$2"
  local expected_binary="${3:-}"
  local existing_pid index
  for index in "${!pending_pids[@]}"; do
    existing_pid="${pending_pids[$index]}"
    if [[ "$existing_pid" == "$pid" ]]; then
      return 0
    fi
  done
  pending_pids+=("$pid")
  pending_labels+=("$label")
  pending_expected_binaries+=("$expected_binary")
}

request_pid_stop() {
  local pid="$1"
  local label="$2"
  local expected_binary="${3:-}"
  local stopped_pid=""

  stopped_pid="$(terminate_pid "$pid" "$label" 0 "$expected_binary" || true)"
  if [[ -z "$stopped_pid" ]]; then
    return 1
  fi
  add_pending_stop "$pid" "$label" "$expected_binary"
  return 0
}

wait_for_pending_stops() {
  local attempt index pid label expected_binary
  local next_pids=() next_labels=() next_expected_binaries=()

  [[ ${#pending_pids[@]} -gt 0 ]] || return 0

  for attempt in {1..20}; do
    next_pids=()
    next_labels=()
    next_expected_binaries=()
    for index in "${!pending_pids[@]}"; do
      pid="${pending_pids[$index]}"
      label="${pending_labels[$index]}"
      expected_binary="${pending_expected_binaries[$index]}"
      if kill -0 "$pid" 2>/dev/null; then
        next_pids+=("$pid")
        next_labels+=("$label")
        next_expected_binaries+=("$expected_binary")
      else
        echo "Stopped $label (PID $pid)."
      fi
    done
    pending_pids=("${next_pids[@]}")
    pending_labels=("${next_labels[@]}")
    pending_expected_binaries=("${next_expected_binaries[@]}")
    [[ ${#pending_pids[@]} -eq 0 ]] && return 0
    sleep 0.5
  done

  for index in "${!pending_pids[@]}"; do
    pid="${pending_pids[$index]}"
    label="${pending_labels[$index]}"
    expected_binary="${pending_expected_binaries[$index]}"
    terminate_pid "$pid" "$label" 1 "$expected_binary" >/dev/null || true
  done

  for attempt in {1..10}; do
    next_pids=()
    next_labels=()
    next_expected_binaries=()
    for index in "${!pending_pids[@]}"; do
      pid="${pending_pids[$index]}"
      label="${pending_labels[$index]}"
      expected_binary="${pending_expected_binaries[$index]}"
      if kill -0 "$pid" 2>/dev/null; then
        next_pids+=("$pid")
        next_labels+=("$label")
        next_expected_binaries+=("$expected_binary")
      else
        echo "Stopped $label (PID $pid) after force kill."
      fi
    done
    pending_pids=("${next_pids[@]}")
    pending_labels=("${next_labels[@]}")
    pending_expected_binaries=("${next_expected_binaries[@]}")
    [[ ${#pending_pids[@]} -eq 0 ]] && return 0
    sleep 0.2
  done

  for index in "${!pending_pids[@]}"; do
    pid="${pending_pids[$index]}"
    label="${pending_labels[$index]}"
    echo "Warning: failed to stop $label (PID $pid)." >&2
  done
}

queue_stop_binary() {
  local binary="$1"
  local pid_file="$run_dir/$binary.pid"
  local port pid fallback_pid

  if [[ -f "$pid_file" ]]; then
    pid="$(tr -d '[:space:]' < "$pid_file")"
    if [[ -n "$pid" ]]; then
      request_pid_stop "$pid" "$binary" "$binary" || true
    fi
    rm -f "$pid_file"
  fi

  port="$(port_for_binary "$binary")"
  while IFS= read -r fallback_pid; do
    [[ -n "$fallback_pid" ]] || continue
    request_pid_stop "$fallback_pid" "$binary listener on :$port" "$binary" || true
  done < <(list_listening_pids_for_port "$port" | awk '!seen[$0]++')
}

cli_mode=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --mode)
      shift
      [[ $# -gt 0 ]] || { echo "Error: --mode requires a value." >&2; exit 1; }
      cli_mode="$1"
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

load_env_file

# Default to `both` when no mode is supplied, so a plain `trench-tools-stop`
# always cleans up every process this workspace could have started. Honouring
# TRENCH_TOOLS_MODE here would silently skip binaries that a previous run
# started under a different one-off mode.
mode="$(validate_mode "${cli_mode:-both}")"

execution_engine_port="${EXECUTION_ENGINE_PORT:-8788}"
launchdeck_port="${LAUNCHDECK_PORT:-8789}"
launchdeck_follow_daemon_port="${LAUNCHDECK_FOLLOW_DAEMON_PORT:-8790}"
data_root="$(resolve_path "${TRENCH_TOOLS_DATA_ROOT:-.local/trench-tools}")"
run_dir="$data_root/run"

mkdir -p "$run_dir"
pending_pids=()
pending_labels=()
pending_expected_binaries=()

echo "Stopping trench tools ($mode)..."
while IFS= read -r binary; do
  [[ -n "$binary" ]] || continue
  queue_stop_binary "$binary"
done < <(target_binaries_for_mode "$mode")
wait_for_pending_stops
