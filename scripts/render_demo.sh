#!/usr/bin/env bash
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd -- "${script_dir}/.." && pwd)"
html_path="${repo_root}/docs/demo/index.html"
output_dir="${OUTPUT_DIR:-${repo_root}/docs/assets}"
gif_path="${output_dir}/telegram-personal-mcp-demo.gif"
mp4_path="${output_dir}/telegram-personal-mcp-demo.mp4"
proof_path="${output_dir}/telegram-personal-mcp-demo-proof.json"

for tool in cargo ffmpeg jq; do
  if ! command -v "${tool}" >/dev/null 2>&1; then
    printf 'missing required demo renderer tool: %s\n' "${tool}" >&2
    exit 1
  fi
done

browser_kind="${DEMO_BROWSER:-}"
browser_bin=""
if [[ -z "${browser_kind}" || "${browser_kind}" == "firefox" ]]; then
  browser_bin="${FIREFOX_BIN:-$(command -v firefox || true)}"
  if [[ -n "${browser_bin}" && -x "${browser_bin}" ]]; then
    browser_kind="firefox"
  fi
fi
if [[ -z "${browser_bin}" && ( -z "${browser_kind}" || "${browser_kind}" == "chromium" ) ]]; then
  browser_bin="${CHROMIUM_BIN:-$(command -v chromium || command -v chromium-browser || command -v google-chrome || true)}"
  if [[ -n "${browser_bin}" && -x "${browser_bin}" ]]; then
    browser_kind="chromium"
  fi
fi
if [[ -z "${browser_bin}" || ! -x "${browser_bin}" ]]; then
  printf 'install Firefox or set FIREFOX_BIN; set DEMO_BROWSER=chromium and CHROMIUM_BIN to override\n' >&2
  exit 1
fi

runtime="$(mktemp -d "${TMPDIR:-/var/tmp}/telegram-personal-mcp-demo.XXXXXX")"
cleanup() {
  rm -rf -- "${runtime}"
}
trap cleanup EXIT

cd "${repo_root}"
proof_json="$(cargo run --locked --quiet --example approval_demo)"

jq -e '
  .proof == "telegram-personal-mcp-agent-workspace-v4" and
  .fixture == "deterministic_no_credentials" and
  .dialog_title == "Project Room" and
  .dialog_chat_ref == "-202" and
  .dialog_canonical_chat_id == "-202" and
  (.dialog_scanned_count | type == "number" and . >= 1) and
  .chat_id == "-202" and
  .canonical_chat_id == "-202" and
  .source_message_id == 22 and
  .source_message == "Production deploy is ready. Should I start now?" and
  .reply_text == "Start at 18:00 UTC. Roll back immediately if any health check fails." and
  .direct_bypass == "blocked" and
  .approval_card_count == 1 and
  .approval_state == "sent" and
  .delivery_count == 1 and
  .backend_readback == "confirmed" and
  .mcp_calls == [
    "search_dialogs",
    "search_messages",
    "read_context",
    "prepare_message",
    "send_prepared",
    "request_send_approval",
    "get_send_approval",
    "read_context"
  ] and
  (.payload_sha256 | test("^[0-9a-f]{64}$")) and
  (.target_message_id | type == "number")
' >/dev/null <<<"${proof_json}"

payload_sha256="$(jq -r '.payload_sha256' <<<"${proof_json}")"
target_message_id="$(jq -r '.target_message_id' <<<"${proof_json}")"
dialog_scanned_count="$(jq -r '.dialog_scanned_count' <<<"${proof_json}")"
html_uri="file://${html_path}"

for scene in 0 1 2 3 4 5 6; do
  scene_uri="${html_uri}?scene=${scene}&hash=${payload_sha256}&target=${target_message_id}&scanned=${dialog_scanned_count}"
  if [[ "${browser_kind}" == "firefox" ]]; then
    mkdir -p "${runtime}/firefox-${scene}"
    "${browser_bin}" \
      --headless \
      --profile "${runtime}/firefox-${scene}" \
      --window-size 1200,676 \
      --screenshot "${runtime}/scene-${scene}.png" \
      "${scene_uri}" \
      >/dev/null 2>&1
  else
    "${browser_bin}" \
      --headless=new \
      --disable-gpu \
      --hide-scrollbars \
      --force-device-scale-factor=1 \
      --window-size=1200,676 \
      --screenshot="${runtime}/scene-${scene}.png" \
      "${scene_uri}" \
      >/dev/null 2>&1
  fi
done

mkdir -p "${output_dir}"
jq -S . <<<"${proof_json}" >"${runtime}/proof.json"
install -m 0644 "${runtime}/proof.json" "${proof_path}"

ffmpeg -v error -y \
  -framerate 30 -loop 1 -t 2.4 -i "${runtime}/scene-0.png" \
  -framerate 30 -loop 1 -t 2.0 -i "${runtime}/scene-1.png" \
  -framerate 30 -loop 1 -t 2.0 -i "${runtime}/scene-2.png" \
  -framerate 30 -loop 1 -t 2.0 -i "${runtime}/scene-3.png" \
  -framerate 30 -loop 1 -t 2.1 -i "${runtime}/scene-4.png" \
  -framerate 30 -loop 1 -t 1.7 -i "${runtime}/scene-5.png" \
  -framerate 30 -loop 1 -t 3.0 -i "${runtime}/scene-6.png" \
  -filter_complex '
    [0:v]setpts=PTS-STARTPTS,format=yuv420p[v0];
    [1:v]setpts=PTS-STARTPTS,format=yuv420p[v1];
    [2:v]setpts=PTS-STARTPTS,format=yuv420p[v2];
    [3:v]setpts=PTS-STARTPTS,format=yuv420p[v3];
    [4:v]setpts=PTS-STARTPTS,format=yuv420p[v4];
    [5:v]setpts=PTS-STARTPTS,format=yuv420p[v5];
    [6:v]setpts=PTS-STARTPTS,format=yuv420p[v6];
    [v0][v1]xfade=transition=fade:duration=0.25:offset=2.15[x1];
    [x1][v2]xfade=transition=fade:duration=0.25:offset=3.90[x2];
    [x2][v3]xfade=transition=fade:duration=0.25:offset=5.65[x3];
    [x3][v4]xfade=transition=fade:duration=0.25:offset=7.40[x4];
    [x4][v5]xfade=transition=fade:duration=0.25:offset=9.25[x5];
    [x5][v6]xfade=transition=fade:duration=0.25:offset=10.70,format=yuv420p[v]
  ' \
  -map '[v]' \
  -an \
  -c:v libx264 \
  -preset slow \
  -crf 20 \
  -movflags +faststart \
  "${mp4_path}"

ffmpeg -v error -y \
  -i "${mp4_path}" \
  -filter_complex '
    [0:v]fps=12,split[s0][s1];
    [s0]palettegen=max_colors=192:stats_mode=diff[p];
    [s1][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle
  ' \
  -loop 0 \
  "${gif_path}"

printf 'generated proof-bound demo assets:\n'
du -h "${proof_path}" "${gif_path}" "${mp4_path}"
