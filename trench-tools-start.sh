#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<'EOF'
Usage: ./trench-tools-start.sh [--mode ee|ld|both]
EOF
}

load_env_file() {
  # Hand-parse the .env file rather than `source`ing it so Windows-authored
  # files (CRLF line endings or quoted values) cannot corrupt variable names
  # or leak a trailing carriage return into exported values.
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

validate_terminal_mode() {
  local normalized
  normalized="$(printf '%s' "${1:-none}" | tr '[:upper:]' '[:lower:]')"
  case "$normalized" in
    none|'')
      printf '%s\n' "none"
      ;;
    logs)
      printf '%s\n' "logs"
      ;;
    *)
      echo "Error: TRENCH_TOOLS_TERMINALS must be none or logs." >&2
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

display_name_for_binary() {
  case "$1" in
    execution-engine) printf '%s\n' "Execution Engine" ;;
    launchdeck-engine) printf '%s\n' "LaunchDeck Engine" ;;
    launchdeck-follow-daemon) printf '%s\n' "Follow Daemon" ;;
    *) printf '%s\n' "$1" ;;
  esac
}

description_for_binary() {
  case "$1" in
    execution-engine) printf '%s\n' "local trading service used by the browser extension" ;;
    launchdeck-engine) printf '%s\n' "LaunchDeck API and control service" ;;
    launchdeck-follow-daemon) printf '%s\n' "background follow-trading worker" ;;
    *) printf '%s\n' "local service" ;;
  esac
}

now_ms() {
  date +%s%3N
}

format_elapsed_ms() {
  local ms="$1"
  if (( ms < 1000 )); then
    printf '%sms\n' "$ms"
  elif (( ms < 60000 )); then
    printf '%s.%ss\n' "$((ms / 1000))" "$(((ms % 1000) / 100))"
  else
    printf '%sm %02ss\n' "$((ms / 60000))" "$(((ms % 60000) / 1000))"
  fi
}

print_step_row() {
  local status="$1"
  local label="$2"
  local detail="${3:-}"
  printf '  %-5s %-28s %s\n' "$status" "$label" "$detail"
}

step_begin() {
  current_step_label="$1"
  current_step_started_at="$(now_ms)"
  print_step_row "${2:-WAIT}" "$current_step_label" "${3:-}"
}

step_ok() {
  local elapsed
  elapsed="$(( $(now_ms) - current_step_started_at ))"
  print_step_row "OK" "$current_step_label" "$(format_elapsed_ms "$elapsed")"
}

print_startup_overview() {
  local mode="$1"
  local logs="$2"
  local mode_description

  case "$mode" in
    ee) mode_description="Execution Engine only" ;;
    ld) mode_description="LaunchDeck only" ;;
    *) mode_description="Execution Engine and LaunchDeck" ;;
  esac

  echo
  echo "Trench Tools startup"
  echo "Mode: $mode ($mode_description)"
  echo "Logs: $logs"
  echo
  echo "Steps"
}

target_specs_for_mode() {
  case "$1" in
    ee)
      printf '%s\n' "execution-engine:execution-engine"
      ;;
    ld)
      printf '%s\n' "launchdeck-engine:launchdeck-engine" "launchdeck-engine:launchdeck-follow-daemon"
      ;;
    both)
      printf '%s\n' \
        "execution-engine:execution-engine" \
        "launchdeck-engine:launchdeck-engine" \
        "launchdeck-engine:launchdeck-follow-daemon"
      ;;
  esac
}

rotate_log() {
  local log_path="$1"
  if [[ -f "$log_path.1" ]]; then
    rm -f "$log_path.1"
  fi
  if [[ -f "$log_path" ]]; then
    mv "$log_path" "$log_path.1"
  fi
}

get_binary_path() {
  local binary="$1"
  printf '%s\n' "$project_root/target/release/$binary"
}

build_targets() {
  local specs=("$@")
  local cargo_args=(build --release)
  local binary_names=()
  local spec binary binary_path

  [[ ${#specs[@]} -gt 0 ]] || return 0

  for spec in "${specs[@]}"; do
    [[ -n "$spec" ]] || continue
    binary="${spec##*:}"
    cargo_args+=(--bin "$binary")
    binary_names+=("$binary")
  done

  step_begin "Build services" "BUILD" "${binary_names[*]}"
  (
    cd "$project_root"
    cargo "${cargo_args[@]}"
  ) || return 1

  for spec in "${specs[@]}"; do
    [[ -n "$spec" ]] || continue
    binary="${spec##*:}"
    binary_path="$(get_binary_path "$binary")"
    if [[ ! -x "$binary_path" ]]; then
      echo "Error: expected built binary at $binary_path." >&2
      return 1
    fi
  done
  step_ok
}

wait_for_health_endpoint() {
  local binary="$1"
  local pid="$2"
  local log_path="$3"
  local url="" body="" max_attempts=120 attempt

  case "$binary" in
    execution-engine)
      url="http://127.0.0.1:$execution_engine_port/api/extension/auth/bootstrap"
      ;;
    launchdeck-engine)
      url="http://127.0.0.1:$launchdeck_port/health"
      ;;
    launchdeck-follow-daemon)
      url="http://127.0.0.1:$launchdeck_follow_daemon_port/health"
      ;;
    *)
      return 0
      ;;
  esac

  for ((attempt = 1; attempt <= max_attempts; attempt++)); do
    body="$(curl -fsS --max-time 2 "$url" 2>/dev/null || true)"
    case "$binary" in
      execution-engine)
        if [[ "$body" == *'"authRequired":true'* || "$body" == *'"authRequired": true'* || "$body" == *'"status":"ready"'* || "$body" == *'"status": "ready"'* ]]; then
          return 0
        fi
        ;;
      *)
        if [[ "$body" == *'"ok":true'* || "$body" == *'"ok": true'* || "$body" == *'"running":true'* || "$body" == *'"running": true'* ]]; then
          return 0
        fi
        ;;
    esac

    if ! kill -0 "$pid" 2>/dev/null; then
      echo "Error: $binary exited before it became healthy. Check $log_path." >&2
      return 1
    fi

    sleep 0.5
  done

  echo "Error: $binary did not become healthy. Check $log_path." >&2
  return 1
}

execution_engine_auth_token_path() {
  local url="http://127.0.0.1:$execution_engine_port/api/extension/auth/bootstrap"
  local body token_path
  body="$(curl -fsS --max-time 2 "$url" 2>/dev/null || true)"
  [[ -n "$body" ]] || return 0
  token_path="$(printf '%s\n' "$body" | sed -n 's/.*"tokenFilePath"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')"
  [[ -n "$token_path" ]] || return 0
  printf '%s\n' "$token_path"
}

print_browser_tunnel_guidance() {
  local manual_tunnel="" check_commands="" forwarded_ports=""

  case "$mode" in
    ee)
      forwarded_ports="8788"
      manual_tunnel="ssh -L 8788:127.0.0.1:8788 root@YOUR_SERVER_IP"
      check_commands="Test-NetConnection 127.0.0.1 -Port 8788"
      ;;
    ld)
      forwarded_ports="8789"
      manual_tunnel="ssh -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP"
      check_commands="Test-NetConnection 127.0.0.1 -Port 8789"
      ;;
    *)
      forwarded_ports="8788 and 8789"
      manual_tunnel="ssh -L 8788:127.0.0.1:8788 -L 8789:127.0.0.1:8789 root@YOUR_SERVER_IP"
      check_commands="Test-NetConnection 127.0.0.1 -Port 8788; Test-NetConnection 127.0.0.1 -Port 8789"
      ;;
  esac

  echo
  echo "Browser tunnel"
  echo "  Remote browser: forward local port(s) $forwarded_ports to this VPS."
  echo "  One-off: $manual_tunnel"
  echo "  Do not expose ports 8788, 8789, or 8790 publicly."
}

print_final_summary() {
  local token_path index binary pid log_path port

  echo
  echo "Trench Tools services are ready."
  if [[ "${TRENCH_TOOLS_FINAL_DIAGNOSTICS:-}" == "1" ]]; then
    print_browser_tunnel_guidance
    return 0
  fi
  echo
  echo "Launched services:"
  for index in "${!started_binaries[@]}"; do
    binary="${started_binaries[$index]}"
    pid="${started_pids[$index]}"
    log_path="${started_logs[$index]}"
    port="$(port_for_binary "$binary")"
    printf '  - %s (%s)\n' "$(display_name_for_binary "$binary")" "$binary"
    printf '    Address: http://127.0.0.1:%s\n' "$port"
    printf '    Process ID: %s\n' "$pid"
    printf '    Log file: %s\n' "$log_path"
  done

  print_browser_tunnel_guidance

  echo
  echo "Extension authentication"
  if [[ "$mode" == "ld" ]]; then
    echo "  - No Execution Engine auth token is needed because you started LaunchDeck only."
    return 0
  fi

  token_path="$(execution_engine_auth_token_path)"
  if [[ -z "$token_path" ]]; then
    echo "  - The Execution Engine started, but the script could not read the auth token file path."
    echo "  - Check the execution-engine log above, then restart this script if needed."
    return 0
  fi

  echo "  Token file: $token_path"
  echo "  Paste this token into the extension. Keep it private."
}

wait_for_started_targets_healthy() {
  local pending_binaries=("${started_binaries[@]}")
  local pending_pids=("${started_pids[@]}")
  local pending_logs=("${started_logs[@]}")
  local next_binaries=() next_pids=() next_logs=()
  local attempt index binary pid log_path body healthy

  [[ ${#pending_binaries[@]} -gt 0 ]] || return 0

  for ((attempt = 1; attempt <= 120; attempt++)); do
    next_binaries=()
    next_pids=()
    next_logs=()
    for index in "${!pending_binaries[@]}"; do
      binary="${pending_binaries[$index]}"
      pid="${pending_pids[$index]}"
      log_path="${pending_logs[$index]}"
      body=""
      healthy=0

      case "$binary" in
        execution-engine)
          body="$(curl -fsS --max-time 2 "http://127.0.0.1:$execution_engine_port/api/extension/auth/bootstrap" 2>/dev/null || true)"
          if [[ "$body" == *'"authRequired":true'* || "$body" == *'"authRequired": true'* || "$body" == *'"status":"ready"'* || "$body" == *'"status": "ready"'* ]]; then
            healthy=1
          fi
          ;;
        launchdeck-engine)
          body="$(curl -fsS --max-time 2 "http://127.0.0.1:$launchdeck_port/health" 2>/dev/null || true)"
          if [[ "$body" == *'"ok":true'* || "$body" == *'"ok": true'* || "$body" == *'"running":true'* || "$body" == *'"running": true'* ]]; then
            healthy=1
          fi
          ;;
        launchdeck-follow-daemon)
          body="$(curl -fsS --max-time 2 "http://127.0.0.1:$launchdeck_follow_daemon_port/health" 2>/dev/null || true)"
          if [[ "$body" == *'"ok":true'* || "$body" == *'"ok": true'* || "$body" == *'"running":true'* || "$body" == *'"running": true'* ]]; then
            healthy=1
          fi
          ;;
        *)
          healthy=1
          ;;
      esac

      if (( healthy )); then
        continue
      fi

      if ! kill -0 "$pid" 2>/dev/null; then
        echo "Error: $binary exited before it became healthy. Check $log_path." >&2
        return 1
      fi

      next_binaries+=("$binary")
      next_pids+=("$pid")
      next_logs+=("$log_path")
    done

    pending_binaries=("${next_binaries[@]}")
    pending_pids=("${next_pids[@]}")
    pending_logs=("${next_logs[@]}")
    [[ ${#pending_binaries[@]} -eq 0 ]] && return 0
    sleep 0.5
  done

  echo "Error: timed out waiting for healthy services. Check ${pending_logs[*]}." >&2
  return 1
}

cleanup_started_binaries() {
  bash "$project_root/trench-tools-stop.sh" --mode "$mode" >/dev/null 2>&1 || true
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

mode="$(validate_mode "${cli_mode:-${TRENCH_TOOLS_MODE:-both}}")"
terminal_mode="$(validate_terminal_mode "${TRENCH_TOOLS_TERMINALS:-none}")"

execution_engine_port="${EXECUTION_ENGINE_PORT:-8788}"
launchdeck_port="${LAUNCHDECK_PORT:-8789}"
launchdeck_follow_daemon_port="${LAUNCHDECK_FOLLOW_DAEMON_PORT:-8790}"
data_root="$(resolve_path "${TRENCH_TOOLS_DATA_ROOT:-.local/trench-tools}")"
log_dir="$(resolve_path "${LOG_DIR:-.local/logs}")"
run_dir="$data_root/run"

export TRENCH_TOOLS_MODE="$mode"
export TRENCH_TOOLS_TERMINALS="$terminal_mode"
export TRENCH_TOOLS_DATA_ROOT="$data_root"
export TRENCH_TOOLS_PROJECT_ROOT="$project_root"
export EXECUTION_ENGINE_PORT="$execution_engine_port"
export LAUNCHDECK_PORT="$launchdeck_port"
export LAUNCHDECK_FOLLOW_DAEMON_PORT="$launchdeck_follow_daemon_port"

: "${LAUNCHDECK_LOCAL_DATA_DIR:=$data_root}"
: "${LAUNCHDECK_SEND_LOG_DIR:=$data_root/send-reports}"
: "${LAUNCHDECK_ENGINE_RUNTIME_PATH:=$data_root/engine-runtime.json}"
: "${LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH:=$data_root/follow-daemon-state.json}"
: "${EXECUTION_ENGINE_BASE_URL:=http://127.0.0.1:$EXECUTION_ENGINE_PORT}"
: "${LAUNCHDECK_FOLLOW_DAEMON_URL:=http://127.0.0.1:$LAUNCHDECK_FOLLOW_DAEMON_PORT}"

if [[ -z "${LAUNCHDECK_EXECUTION_ENGINE_BASE_URL:-}" ]]; then
  if [[ "$mode" == "ld" ]]; then
    unset LAUNCHDECK_EXECUTION_ENGINE_BASE_URL
  else
    export LAUNCHDECK_EXECUTION_ENGINE_BASE_URL="$EXECUTION_ENGINE_BASE_URL"
  fi
fi

export LAUNCHDECK_LOCAL_DATA_DIR
export LAUNCHDECK_SEND_LOG_DIR
export LAUNCHDECK_ENGINE_RUNTIME_PATH
export LAUNCHDECK_FOLLOW_DAEMON_STATE_PATH
export EXECUTION_ENGINE_BASE_URL
if [[ -n "${LAUNCHDECK_EXECUTION_ENGINE_BASE_URL:-}" ]]; then
  export LAUNCHDECK_EXECUTION_ENGINE_BASE_URL
fi
export LAUNCHDECK_FOLLOW_DAEMON_URL

mkdir -p "$run_dir" "$log_dir" "$LAUNCHDECK_SEND_LOG_DIR"

if [[ "$terminal_mode" == "logs" ]]; then
  echo "Warning: TRENCH_TOOLS_TERMINALS=logs is currently only supported by the Windows PowerShell launcher; continuing headless." >&2
fi

mapfile -t target_specs < <(target_specs_for_mode "$mode")
print_startup_overview "$mode" "$log_dir" "${target_specs[@]}"

step_begin "Stop old processes" "WAIT" "mode $mode"
bash "$project_root/trench-tools-stop.sh" --mode "$mode" >/dev/null 2>&1 || true
step_ok

started_binaries=()
started_pids=()
start_failed=0
started_logs=()

if ! build_targets "${target_specs[@]}"; then
  exit 1
fi

echo
echo "Launched"
step_begin "Launch services" "WAIT"
for spec in "${target_specs[@]}"; do
  [[ -n "$spec" ]] || continue
  binary="${spec##*:}"
  log_path="$log_dir/$binary.log"
  binary_path="$(get_binary_path "$binary")"
  rotate_log "$log_path"

  (
    cd "$project_root"
    nohup "$binary_path" >>"$log_path" 2>&1 </dev/null &
    printf '%s\n' "$!" > "$run_dir/$binary.pid"
  )
  pid="$(tr -d '[:space:]' < "$run_dir/$binary.pid")"
  started_binaries+=("$binary")
  started_pids+=("$pid")
  started_logs+=("$log_path")
  printf '  OK    %-24s http://127.0.0.1:%-5s pid %-8s logs %s\n' \
    "$(display_name_for_binary "$binary")" \
    "$(port_for_binary "$binary")" \
    "$pid" \
    "$log_path"
done
step_ok

echo
step_begin "Wait for readiness" "WAIT"
if ! wait_for_started_targets_healthy; then
  start_failed=1
else
  step_ok
fi

if (( start_failed )); then
  cleanup_started_binaries
  exit 1
fi

print_final_summary
