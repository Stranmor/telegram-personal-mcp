#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
binary_path="${repo_root}/target/debug/telegram-personal-mcp"

demo_prompt='Alex sent me a contract in Telegram. Find it, read the surrounding context, add "% reviewed by Codex" to a copy named contract-reviewed.pdf, reply to the source message with the caption "Reviewed contract", and confirm the reply. Keep the original unchanged. Demo fixture: no real message will be sent.'

present_codex_events() {
  local reset=$'\033[0m'
  local cyan=$'\033[38;5;81m'
  local blue=$'\033[38;5;75m'
  local green=$'\033[38;5;114m'
  local amber=$'\033[38;5;215m'
  local muted=$'\033[38;5;245m'
  local event
  local event_type
  local item_type
  local phase
  local tool
  local showed_copy=0
  local showed_hash=0
  local delivery_confirmed=0
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
          search_messages)
            local query
            query="$(jq -r '.item.arguments.query // ""' <<<"${event}")"
            printf '%bCodex -> Telegram MCP%b  search_messages(query="%s")\n' \
              "${blue}" "${reset}" "${query}"
            ;;
          read_context)
            local message_id
            message_id="$(jq -r '.item.arguments.message_id // "?"' <<<"${event}")"
            if ((delivery_confirmed == 0)); then
              printf '%bCodex -> Telegram MCP%b  read_context(message=%s)\n' \
                "${blue}" "${reset}" "${message_id}"
            else
              printf '%bCodex -> Telegram MCP%b  read_context(message=%s) %b[verify]%b\n' \
                "${blue}" "${reset}" "${message_id}" "${muted}" "${reset}"
            fi
            ;;
          download_attachment)
            local message_id
            message_id="$(jq -r '.item.arguments.message_id // "?"' <<<"${event}")"
            printf '%bCodex -> Telegram MCP%b  download_attachment(message=%s)\n' \
              "${blue}" "${reset}" "${message_id}"
            ;;
          send_file)
            local reply_to
            local caption
            local file_path
            local approved_hash
            reply_to="$(jq -r '.item.arguments.reply_to_message_id // "?"' <<<"${event}")"
            caption="$(jq -r '.item.arguments.caption // ""' <<<"${event}")"
            file_path="$(jq -r '.item.arguments.file_path // "" | split("/") | last' <<<"${event}")"
            approved_hash="$(jq -r '.item.arguments.approved_sha256 // ""' <<<"${event}")"
            if ((showed_copy == 0)) && [[ "${file_path}" == "contract-reviewed.pdf" ]]; then
              printf '%bCodex -> workspace%b     create writable copy: contract-reviewed.pdf\n' \
                "${cyan}" "${reset}"
              printf '        %boriginal remains immutable%b\n' "${green}" "${reset}"
              showed_copy=1
              sleep 0.45
            fi
            if ((showed_hash == 0)) && [[ "${approved_hash}" =~ ^[0-9a-f]{64}$ ]]; then
              printf '%bCodex -> local%b         sha256(contract-reviewed.pdf)\n' \
                "${cyan}" "${reset}"
              printf '        %b%s...%s%b\n' \
                "${green}" "${approved_hash:0:14}" "${approved_hash: -8}" "${reset}"
              showed_hash=1
              sleep 0.45
            fi
            printf '%bCodex -> Telegram MCP%b  send_file(reply_to=%s, caption="%s")\n' \
              "${blue}" "${reset}" "${reply_to}" "${caption}"
            ;;
        esac
        sleep 0.35
        continue
      fi

      if [[ "${phase}" == "item.completed" ]]; then
        local tool_status
        tool_status="$(jq -r '.item.status // ""' <<<"${event}")"
        if [[ "${tool_status}" != "completed" ]]; then
          case "${tool}" in
            search_messages)
              printf '        %bno match; refining the search%b\n' "${amber}" "${reset}"
              ;;
            read_context)
              printf '        %bretrying the context read%b\n' "${amber}" "${reset}"
              ;;
            send_file)
              printf '        %bretrying with corrected file binding%b\n' "${amber}" "${reset}"
              ;;
            *)
              printf '        %bretrying%b\n' "${amber}" "${reset}"
              ;;
          esac
          continue
        fi

        case "${tool}" in
          search_messages)
            local found_id
            local found_text
            found_id="$(jq -r '.item.result.structured_content.data.messages[0].message_id // "?"' <<<"${event}")"
            found_text="$(jq -r '.item.result.structured_content.data.messages[0].text // ""' <<<"${event}")"
            if [[ "${found_id}" == "?" ]]; then
              printf '        %bno matching messages; refining the search%b\n' \
                "${amber}" "${reset}"
            else
              printf '        %bfound message #%s%b  %s\n' \
                "${green}" "${found_id}" "${reset}" "${found_text}"
            fi
            ;;
          read_context)
            if ((delivery_confirmed == 0)); then
              local prior_text
              prior_text="$(jq -r '.item.result.structured_content.data.older[0].text // "context loaded"' <<<"${event}")"
              printf '        %bcontext confirmed%b  %s\n' \
                "${green}" "${reset}" "${prior_text}"
            else
              local reply_id
              reply_id="$(jq -r '[.item.result.structured_content.data.newer[]? | select(.outgoing == true)][0].message_id // .item.result.structured_content.data.target.message_id // "?"' <<<"${event}")"
              printf '        %breply #%s confirmed in context%b\n' \
                "${green}" "${reply_id}" "${reset}"
            fi
            ;;
          download_attachment)
            local file_name
            local bytes
            local download_hash
            file_name="$(jq -r '.item.result.structured_content.data.attachment.file_name // "attachment"' <<<"${event}")"
            bytes="$(jq -r '.item.result.structured_content.data.bytes_written // "?"' <<<"${event}")"
            download_hash="$(jq -r '.item.result.structured_content.data.sha256 // ""' <<<"${event}")"
            printf '        %b%s downloaded%b  %s bytes  sha256 %s...%s\n' \
              "${green}" "${file_name}" "${reset}" "${bytes}" \
              "${download_hash:0:10}" "${download_hash: -6}"
            ;;
          send_file)
            local sent_status
            local sent_id
            local approved_hash
            local sent_hash
            sent_status="$(jq -r '.item.result.structured_content.data.status // ""' <<<"${event}")"
            sent_id="$(jq -r '.item.result.structured_content.data.message_id // "?"' <<<"${event}")"
            approved_hash="$(jq -r '.item.arguments.approved_sha256 // ""' <<<"${event}")"
            sent_hash="$(jq -r '.item.result.structured_content.data.sha256 // ""' <<<"${event}")"
            if [[ "${sent_status}" == "sent" && -n "${approved_hash}" && "${approved_hash}" == "${sent_hash}" ]]; then
              printf '        %bstatus=sent%b  message #%s  %bSHA-256 MATCH%b\n' \
                "${green}" "${reset}" "${sent_id}" "${green}" "${reset}"
              delivery_confirmed=1
            else
              printf '        %bdelivery verification failed%b\n' "${amber}" "${reset}"
              saw_failure=1
            fi
            ;;
        esac
        sleep 0.55
        continue
      fi
    fi

    if [[ "${event_type}" == "item.completed" && "${item_type}" == "file_change" ]]; then
      local changed_path
      local change_status
      changed_path="$(jq -r '.item.changes[0].path // "" | split("/") | last' <<<"${event}")"
      change_status="$(jq -r '.item.status // ""' <<<"${event}")"
      if ((showed_copy == 0)) && [[ "${change_status}" == "completed" && "${changed_path}" == "contract-reviewed.pdf" ]]; then
        printf '%bCodex -> workspace%b     create writable copy: contract-reviewed.pdf\n' \
          "${cyan}" "${reset}"
        printf '        %boriginal remains immutable%b\n' "${green}" "${reset}"
        showed_copy=1
        sleep 0.55
      fi
      continue
    fi

    if [[ "${event_type}" == "item.completed" && "${item_type}" == "command_execution" ]]; then
      local command_text
      command_text="$(jq -r '.item.command // ""' <<<"${event}")"
      if [[ "${command_text}" == *"sha256sum"* && "${command_text}" == *"contract-reviewed.pdf"* ]]; then
        local command_output
        local reviewed_hash
        command_output="$(jq -r '.item.aggregated_output // ""' <<<"${event}")"
        reviewed_hash="$(awk 'length($1) == 64 && $1 ~ /^[0-9a-f]+$/ && /contract-reviewed\.pdf/ {print $1; exit}' <<<"${command_output}")"
        if ((showed_hash == 0)) && [[ "${reviewed_hash}" =~ ^[0-9a-f]{64}$ ]]; then
          printf '%bCodex -> local%b         sha256(contract-reviewed.pdf)\n' \
            "${cyan}" "${reset}"
          printf '        %b%s...%s%b\n' \
            "${green}" "${reviewed_hash:0:14}" "${reviewed_hash: -8}" "${reset}"
          showed_hash=1
          sleep 0.55
        fi
      fi
      continue
    fi

    if [[ "${event_type}" == "error" || "${event_type}" == "turn.failed" ]]; then
      saw_failure=1
    fi
  done

  if ((saw_failure != 0)); then
    return 1
  fi
}

play_demo() {
  cd "${repo_root}"

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

  runtime="$(mktemp -d "${TMPDIR:-/var/tmp}/telegram-personal-mcp-codex.XXXXXX")"
  events_file="${runtime}/events.jsonl"
  stderr_file="${runtime}/codex.stderr"

  cleanup() {
    printf '\033[?25h'
    if [[ -n "${runtime:-}" && -d "${runtime}" ]]; then
      rm -rf -- "${runtime}"
    fi
  }
  trap cleanup EXIT

  printf '\033[2J\033[H\033[?25l'
  printf '%b%bTelegram Personal MCP%b\n' "${bold}" "${cyan}" "${reset}"
  printf '%bLive Codex agent session -> real MCP-over-stdio calls%b\n\n' \
    "${muted}" "${reset}"
  printf '%bYOU%b\n' "${green}" "${reset}"
  fold -s -w 104 <<<"${demo_prompt}"
  printf '\n'
  sleep 1
  printf '%bCODEX%b\n' "${cyan}" "${reset}"

  mapfile -t existing_servers < <(codex mcp list --json | jq -r '.[].name')
  codex_config=(-c 'approval_policy="never"' -c 'model_reasoning_effort="medium"')
  local server_name
  for server_name in "${existing_servers[@]}"; do
    [[ "${server_name}" =~ ^[A-Za-z0-9_-]+$ ]] || continue
    codex_config+=(-c "mcp_servers.${server_name}.enabled=false")
  done
  codex_config+=(
    -c "mcp_servers.telegram_personal_demo.command=\"${binary_path}\""
    -c "mcp_servers.telegram_personal_demo.args=[\"--demo\",\"--attachment-cache-dir\",\"${runtime}/attachments\",\"--state-db-path\",\"${runtime}/state.sqlite\"]"
    -c 'mcp_servers.telegram_personal_demo.required=true'
    -c 'mcp_servers.telegram_personal_demo.enabled_tools=["health","search_messages","read_context","download_attachment","send_file"]'
    -c 'mcp_servers.telegram_personal_demo.default_tools_approval_mode="approve"'
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
    "For a public demo, act immediately and keep internal setup out of the response. ${demo_prompt}" \
    2>"${stderr_file}" \
    | tee "${events_file}" \
    | present_codex_events
  local -a pipeline_status=("${PIPESTATUS[@]}")
  set -e
  codex_status="${pipeline_status[0]}"
  presenter_status="${pipeline_status[2]}"

  if ((codex_status != 0 || presenter_status != 0)); then
    printf '\n%bLive Codex demo failed; inspect the local run before publishing.%b\n' \
      "${red}" "${reset}"
    exit 1
  fi

  local required_tool
  for required_tool in search_messages read_context download_attachment send_file; do
    jq -e --arg tool "${required_tool}" \
      'select(.type == "item.completed" and .item.type == "mcp_tool_call" and .item.tool == $tool and .item.status == "completed")' \
      "${events_file}" >/dev/null
  done
  [[ "$(jq -s '[.[] | select(.type == "item.completed" and .item.type == "mcp_tool_call" and .item.tool == "read_context" and .item.status == "completed")] | length' "${events_file}")" -ge 2 ]]
  jq -e \
    'select(.type == "item.completed" and .item.type == "mcp_tool_call" and .item.tool == "send_file" and .item.result.structured_content.data.status == "sent" and .item.arguments.approved_sha256 == .item.result.structured_content.data.sha256)' \
    "${events_file}" >/dev/null

  printf '\n%b%bDONE%b  Codex completed the Telegram workflow through MCP.\n' \
    "${bold}" "${green}" "${reset}"
  printf '%bDemo fixture only — no credentials, Telegram network, or real message.%b\n' \
    "${muted}" "${reset}"

  sleep 3
  cleanup
  trap - EXIT
}

if [[ "${1:-}" == "--play" ]]; then
  play_demo
  exit 0
fi

if [[ "${1:-}" == "--present" ]]; then
  [[ -n "${2:-}" && -f "${2}" ]]
  present_codex_events <"${2}"
  exit 0
fi

cd "${repo_root}"

for tool in asciinema ffmpeg cargo codex fold jq; do
  if ! command -v "${tool}" >/dev/null 2>&1; then
    printf 'missing required tool: %s\n' "${tool}" >&2
    exit 1
  fi
done

agg_bin="${AGG_BIN:-}"
if [[ -z "${agg_bin}" ]]; then
  agg_bin="$(command -v agg || true)"
fi
if [[ -z "${agg_bin}" || ! -x "${agg_bin}" ]]; then
  printf 'set AGG_BIN to an executable asciinema/agg binary\n' >&2
  exit 1
fi

output_dir="${OUTPUT_DIR:-${repo_root}/docs/assets}"
cast_path="${output_dir}/telegram-personal-mcp-demo.cast"
gif_path="${output_dir}/telegram-personal-mcp-demo.gif"
mp4_path="${output_dir}/telegram-personal-mcp-demo.mp4"
mkdir -p "${output_dir}"

cargo build --locked --quiet

asciinema record \
  --headless \
  --return \
  --overwrite \
  --output-format asciicast-v2 \
  --window-size 112x31 \
  --idle-time-limit 1.2 \
  --title 'Telegram Personal MCP live Codex workflow' \
  --command "TERM=xterm-256color bash '${BASH_SOURCE[0]}' --play" \
  "${cast_path}"

"${agg_bin}" \
  --quiet \
  --theme github-dark \
  --font-family 'Noto Sans Mono' \
  --font-size 17 \
  --line-height 1.25 \
  --idle-time-limit 1.2 \
  --fps-cap 30 \
  --last-frame-duration 4 \
  --cols 112 \
  --rows 31 \
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

printf 'generated live Codex demo assets:\n'
du -h "${cast_path}" "${gif_path}" "${mp4_path}"
