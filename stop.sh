#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
engine_manifest_path="$(cd "$project_root/rust/launchdeck-engine" && pwd)/Cargo.toml"

get_configured_numeric_setting() {
  local default_value="$1"
  shift
  local variable_names=("$@")
  local file_name file_path variable_name value

  for file_name in ".env" ".env.local" ".env.example"; do
    file_path="$project_root/$file_name"
    [[ -f "$file_path" ]] || continue
    for variable_name in "${variable_names[@]}"; do
      value="$(awk -F= -v key="$variable_name" '
        $1 ~ "^[[:space:]]*" key "[[:space:]]*$" {
          gsub(/^[[:space:]]+|[[:space:]]+$/, "", $2)
          if ($2 ~ /^[0-9]+$/) {
            print $2
            exit
          }
        }
      ' "$file_path")"
      if [[ -n "$value" ]]; then
        printf '%s\n' "$value"
        return
      fi
    done
  done

  printf '%s\n' "$default_value"
}

get_configured_engine_port() {
  get_configured_numeric_setting 8789 "LAUNCHDECK_PORT"
}

get_configured_follow_daemon_port() {
  get_configured_numeric_setting 8790 "LAUNCHDECK_FOLLOW_DAEMON_PORT"
}

stop_launchdeck_process() {
  local pid="$1"
  local reason="$2"

  [[ "$pid" =~ ^[0-9]+$ ]] || return 0
  [[ "$pid" != "$$" ]] || return 0
  if kill -0 "$pid" 2>/dev/null; then
    if kill -TERM "$pid" 2>/dev/null; then
      sleep 0.2
    fi
    if kill -0 "$pid" 2>/dev/null; then
      kill -KILL "$pid" 2>/dev/null || true
    fi
    echo "Stopped process $pid ($reason)."
  fi
}

list_listening_pids_for_port() {
  local port="$1"

  if command -v ss >/dev/null 2>&1; then
    ss -ltnp "( sport = :$port )" 2>/dev/null \
      | awk 'match($0, /pid=([0-9]+)/, m) { print m[1] }'
    return
  fi

  if command -v lsof >/dev/null 2>&1; then
    lsof -tiTCP:"$port" -sTCP:LISTEN 2>/dev/null || true
    return
  fi
}

stop_processes_listening_on_port() {
  local port="$1"
  local label="$2"
  local pid

  while IFS= read -r pid; do
    [[ -n "$pid" ]] || continue
    stop_launchdeck_process "$pid" "$label listener on port $port"
  done < <(list_listening_pids_for_port "$port" | awk '!seen[$0]++')
}

stop_launchdeck_runtime() {
  local known_count=0 pid engine_port follow_daemon_port

  if command -v pgrep >/dev/null 2>&1; then
    while IFS= read -r pid; do
      [[ -n "$pid" ]] || continue
      stop_launchdeck_process "$pid" "existing LaunchDeck runtime"
      known_count=$((known_count + 1))
    done < <(pgrep -af "launchdeck-engine|launchdeck-follow-daemon|$engine_manifest_path" \
      | awk -v self="$$" '$1 != self { print $1 }' \
      | awk '!seen[$0]++')
  fi

  engine_port="$(get_configured_engine_port)"
  follow_daemon_port="$(get_configured_follow_daemon_port)"
  stop_processes_listening_on_port "$engine_port" "LaunchDeck engine"
  stop_processes_listening_on_port "$follow_daemon_port" "LaunchDeck follow daemon"

  if [[ "$known_count" -eq 0 ]]; then
    echo "No running LaunchDeck engine or follow-daemon processes were found."
  else
    echo "LaunchDeck runtime stopped."
  fi
}

cd "$project_root"
stop_launchdeck_runtime
