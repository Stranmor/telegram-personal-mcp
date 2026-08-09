#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
binary_path="${repo_root}/target/debug/telegram-personal-mcp"

live_prompt='Verify my live Telegram connection. Call health, then use the returned positive user_id as chat_id to read at most one recent Saved Messages item. Do not quote, summarize, or expose any account, chat, or message data. Do not call a send tool.'

session_count() {
  "${binary_path}" session list | jq -r '.candidate_count'
}

credentials_ready() {
  [[ "${TELEGRAM_API_ID:-}" =~ ^[1-9][0-9]*$ && -n "${TELEGRAM_API_HASH:-}" ]]
}

prompt_runtime_credentials() {
  if credentials_ready; then
    return 0
  fi
  if [[ ! -t 0 || ! -t 2 ]]; then
    printf 'credential entry requires an interactive terminal\n' >&2
    return 1
  fi

  local api_id
  local api_hash
  read -r -p 'Telegram API ID: ' api_id
  read -r -s -p 'Telegram API hash: ' api_hash
  printf '\n' >&2
  if [[ ! "${api_id}" =~ ^[1-9][0-9]*$ || -z "${api_hash}" ]]; then
    api_hash=''
    printf 'Telegram API ID/hash are invalid or empty\n' >&2
    return 1
  fi
  export TELEGRAM_API_ID="${api_id}"
  export TELEGRAM_API_HASH="${api_hash}"
  api_hash=''
}

print_status() {
  local count
  local api_id_status=absent
  local api_hash_status=absent
  local session_status=absent

  count="$(session_count)"
  [[ "${TELEGRAM_API_ID:-}" =~ ^[1-9][0-9]*$ ]] && api_id_status=present
  [[ -n "${TELEGRAM_API_HASH:-}" ]] && api_hash_status=present
  ((count > 0)) && session_status=present

  jq -n \
    --arg api_id "${api_id_status}" \
    --arg api_hash "${api_hash_status}" \
    --arg session "${session_status}" \
    --argjson session_count "${count}" \
    '{api_id:$api_id,api_hash:$api_hash,session:$session,session_count:$session_count}'
}

require_credentials() {
  if ! credentials_ready; then
    printf 'TELEGRAM_API_ID and TELEGRAM_API_HASH must be present in the environment.\n' >&2
    return 1
  fi
}

require_session() {
  local count
  count="$(session_count)"
  if ((count == 0)); then
    printf 'no project session is configured; run %s login from an interactive terminal\n' \
      "${BASH_SOURCE[0]}" >&2
    return 1
  fi
  if ((count > 1)); then
    printf 'multiple project sessions are visible; selection is intentionally fail-closed\n' >&2
    return 1
  fi
}

login_interactive() (
  if [[ ! -t 0 || ! -t 2 ]]; then
    printf 'login must run directly in an interactive terminal; never pipe phone, OTP, or 2FA values\n' >&2
    return 1
  fi
  require_credentials
  cargo build --locked --quiet --manifest-path "${repo_root}/Cargo.toml"

  local report_file
  report_file="$(mktemp)"
  trap 'rm -f -- "${report_file}"' EXIT

  "${binary_path}" auth login >"${report_file}"
  jq -e '.status == "authenticated" or .status == "already_authorized"' \
    "${report_file}" >/dev/null
  jq -n '{status:"ready",account:"redacted",session:"saved"}'
)

doctor_redacted() (
  require_credentials
  require_session

  local report_file
  local stderr_file
  report_file="$(mktemp)"
  stderr_file="$(mktemp)"
  trap 'rm -f -- "${report_file}" "${stderr_file}"' EXIT

  if ! "${binary_path}" doctor >"${report_file}" 2>"${stderr_file}"; then
    printf 'live Telegram readiness check failed; account details remain redacted\n' >&2
    return 1
  fi
  jq -e '.status == "ready" and (.user_id | type == "number")' "${report_file}" >/dev/null
  jq -n '{status:"ready",account:"redacted",session:"loaded",telegram:"reachable"}'
)

present_live_events() {
  local reset=$'\033[0m'
  local blue=$'\033[38;5;75m'
  local green=$'\033[38;5;114m'
  local amber=$'\033[38;5;215m'
  local event
  local event_type
  local item_type
  local phase
  local tool
  local saw_failure=0

  while IFS= read -r event; do
    [[ -n "${event}" ]] || continue
    jq -e . >/dev/null 2>&1 <<<"${event}" || continue

    event_type="$(jq -r '.type // ""' <<<"${event}")"
    item_type="$(jq -r '.item.type // ""' <<<"${event}")"
    phase="$(jq -r '.type // ""' <<<"${event}")"

    if [[ "${item_type}" == "mcp_tool_call" ]]; then
      tool="$(jq -r '.item.tool // ""' <<<"${event}")"
      if [[ "${phase}" == "item.started" ]]; then
        case "${tool}" in
          health)
            printf '%bCodex -> Telegram MCP%b  health()\n' "${blue}" "${reset}"
            ;;
          read_chat)
            printf '%bCodex -> Telegram MCP%b  read_chat(self, limit=1) %b[content redacted]%b\n' \
              "${blue}" "${reset}" "${amber}" "${reset}"
            ;;
          *)
            printf 'unexpected MCP tool requested in read-only live demo\n' >&2
            saw_failure=1
            ;;
        esac
        sleep 0.45
        continue
      fi

      if [[ "${phase}" == "item.completed" ]]; then
        local status
        status="$(jq -r '.item.status // ""' <<<"${event}")"
        if [[ "${status}" != "completed" ]]; then
          printf '        %blive call failed%b\n' "${amber}" "${reset}"
          saw_failure=1
          continue
        fi
        case "${tool}" in
          health)
            local live_status
            live_status="$(jq -r '.item.result.structured_content.data.status // ""' <<<"${event}")"
            if [[ "${live_status}" == "active" ]]; then
              printf '        %blive account ready%b  identity redacted\n' "${green}" "${reset}"
            else
              printf '        %baccount is not active%b\n' "${amber}" "${reset}"
              saw_failure=1
            fi
            ;;
          read_chat)
            local returned_count
            returned_count="$(jq -r '.item.result.structured_content.data.returned_count // 0' <<<"${event}")"
            printf '        %b%s message metadata item(s) received%b  content redacted\n' \
              "${green}" "${returned_count}" "${reset}"
            ;;
        esac
        sleep 0.65
        continue
      fi
    fi

    if [[ "${event_type}" == "error" || "${event_type}" == "turn.failed" ]]; then
      saw_failure=1
    fi
  done

  ((saw_failure == 0))
}

play_live_demo() {
  cd "${repo_root}"
  require_credentials
  require_session

  local reset=$'\033[0m'
  local bold=$'\033[1m'
  local cyan=$'\033[38;5;81m'
  local green=$'\033[38;5;114m'
  local red=$'\033[38;5;203m'
  local muted=$'\033[38;5;245m'
  local runtime
  local events_file
  local stderr_file
  local codex_status
  local presenter_status
  local -a codex_config
  local -a existing_servers

  runtime="$(mktemp -d "${TMPDIR:-/var/tmp}/telegram-personal-mcp-live.XXXXXX")"
  events_file="${runtime}/events.jsonl"
  stderr_file="${runtime}/codex.stderr"

  cleanup_live() {
    printf '\033[?25h'
    if [[ -n "${runtime:-}" && -d "${runtime}" ]]; then
      rm -rf -- "${runtime}"
    fi
  }
  trap cleanup_live EXIT

  printf '\033[2J\033[H\033[?25l'
  printf '%b%bTelegram Personal MCP — Live Account%b\n' "${bold}" "${cyan}" "${reset}"
  printf '%bReal Telegram session | read-only proof | identity and content redacted%b\n\n' \
    "${muted}" "${reset}"
  printf '%bYOU%b\n' "${green}" "${reset}"
  fold -s -w 100 <<<"${live_prompt}"
  printf '\n%bCODEX%b\n' "${cyan}" "${reset}"

  mapfile -t existing_servers < <(codex mcp list --json | jq -r '.[].name')
  codex_config=(-c 'approval_policy="never"' -c 'model_reasoning_effort="max"')
  local server_name
  for server_name in "${existing_servers[@]}"; do
    [[ "${server_name}" =~ ^[A-Za-z0-9_-]+$ ]] || continue
    codex_config+=(-c "mcp_servers.${server_name}.enabled=false")
  done
  codex_config+=(
    -c "mcp_servers.telegram_personal_live.command=\"${binary_path}\""
    -c "mcp_servers.telegram_personal_live.args=[\"--attachment-cache-dir\",\"${runtime}/attachments\",\"--state-db-path\",\"${runtime}/state.sqlite\"]"
    -c 'mcp_servers.telegram_personal_live.env_vars=["TELEGRAM_API_ID","TELEGRAM_API_HASH","TELEGRAM_MCP_SESSION_PATH","TELEGRAM_MCP_SESSION_CACHE_DIR","TELEGRAM_MCP_PROXY"]'
    -c 'mcp_servers.telegram_personal_live.required=true'
    -c 'mcp_servers.telegram_personal_live.enabled_tools=["health","read_chat"]'
    -c 'mcp_servers.telegram_personal_live.default_tools_approval_mode="approve"'
  )

  set +e
  codex exec \
    --ephemeral \
    --skip-git-repo-check \
    --sandbox workspace-write \
    --color never \
    --json \
    "${codex_config[@]}" \
    -C "${runtime}" \
    "For a public proof, act immediately and never expose private values. ${live_prompt}" \
    2>"${stderr_file}" \
    | tee "${events_file}" \
    | present_live_events
  local -a pipeline_status=("${PIPESTATUS[@]}")
  set -e
  codex_status="${pipeline_status[0]}"
  presenter_status="${pipeline_status[2]}"

  if ((codex_status != 0 || presenter_status != 0)); then
    printf '\n%bLive read-only demo failed; raw account data was not persisted.%b\n' \
      "${red}" "${reset}"
    exit 1
  fi

  jq -e \
    'select(.type == "item.completed" and .item.type == "mcp_tool_call" and .item.tool == "health" and .item.status == "completed" and .item.result.structured_content.data.status == "active")' \
    "${events_file}" >/dev/null
  jq -e \
    'select(.type == "item.completed" and .item.type == "mcp_tool_call" and .item.tool == "read_chat" and .item.status == "completed")' \
    "${events_file}" >/dev/null
  if jq -e \
    'select(.item.type == "mcp_tool_call" and (.item.tool != "health" and .item.tool != "read_chat"))' \
    "${events_file}" >/dev/null; then
    printf 'unexpected MCP tool appeared in the live trace\n' >&2
    exit 1
  fi

  printf '\n%b%bLIVE READ VERIFIED%b  Telegram account and Saved Messages are reachable.\n' \
    "${bold}" "${green}" "${reset}"
  printf '%bNo identity, chat reference, or message content is present in this recording.%b\n' \
    "${muted}" "${reset}"
  sleep 3
  cleanup_live
  trap - EXIT
}

record_live_demo() {
  cd "${repo_root}"
  for tool in asciinema cargo codex ffmpeg fold jq; do
    command -v "${tool}" >/dev/null 2>&1 || {
      printf 'missing required tool: %s\n' "${tool}" >&2
      return 1
    }
  done
  require_credentials
  require_session
  doctor_redacted >/dev/null
  cargo build --locked --quiet

  local agg_bin
  agg_bin="${AGG_BIN:-$(command -v agg || true)}"
  if [[ -z "${agg_bin}" && -x "${repo_root}/target/tools/agg" ]]; then
    agg_bin="${repo_root}/target/tools/agg"
  fi
  [[ -n "${agg_bin}" && -x "${agg_bin}" ]] || {
    printf 'set AGG_BIN to an executable asciinema/agg binary\n' >&2
    return 1
  }

  local output_dir
  local cast_path
  local gif_path
  local mp4_path
  output_dir="${LIVE_DEMO_OUTPUT_DIR:-${repo_root}/target/live-auth-demo}"
  cast_path="${output_dir}/telegram-personal-mcp-live.cast"
  gif_path="${output_dir}/telegram-personal-mcp-live.gif"
  mp4_path="${output_dir}/telegram-personal-mcp-live.mp4"
  mkdir -p "${output_dir}"

  asciinema record \
    --headless \
    --return \
    --overwrite \
    --output-format asciicast-v2 \
    --window-size 106x24 \
    --idle-time-limit 1.2 \
    --title 'Telegram Personal MCP live account proof' \
    --command "TERM=xterm-256color bash '${BASH_SOURCE[0]}' --play-live" \
    "${cast_path}"

  "${agg_bin}" \
    --quiet \
    --theme github-dark \
    --font-family 'Noto Sans Mono' \
    --font-size 18 \
    --line-height 1.25 \
    --idle-time-limit 1.2 \
    --fps-cap 30 \
    --last-frame-duration 4 \
    --cols 106 \
    --rows 24 \
    "${cast_path}" \
    "${gif_path}"

  ffmpeg \
    -hide_banner \
    -loglevel error \
    -y \
    -ignore_loop 1 \
    -i "${gif_path}" \
    -vf 'fps=30,scale=trunc(iw/2)*2:trunc(ih/2)*2' \
    -c:v libx264 \
    -preset medium \
    -crf 22 \
    -pix_fmt yuv420p \
    -movflags +faststart \
    -an \
    "${mp4_path}"

  printf 'private live-auth preview generated under %s\n' "${output_dir}"
  du -h "${cast_path}" "${gif_path}" "${mp4_path}"
}

login_and_record() (
  prompt_runtime_credentials
  login_interactive
  record_live_demo
)

case "${1:-status}" in
  status)
    cargo build --locked --quiet --manifest-path "${repo_root}/Cargo.toml"
    print_status
    ;;
  login)
    login_interactive
    ;;
  doctor)
    cargo build --locked --quiet --manifest-path "${repo_root}/Cargo.toml"
    doctor_redacted
    ;;
  record-read-only)
    record_live_demo
    ;;
  login-and-record)
    login_and_record
    ;;
  --play-live)
    play_live_demo
    ;;
  --present-live)
    present_live_events
    ;;
  *)
    printf 'usage: %s [status|login|doctor|record-read-only|login-and-record]\n' \
      "${BASH_SOURCE[0]}" >&2
    exit 2
    ;;
esac
