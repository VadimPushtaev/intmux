#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

status=0

check_max_lines() {
  local limit="$1"
  shift

  local file
  while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    local lines
    lines="$(wc -l < "$file")"
    if (( lines > limit )); then
      echo "file too large: $file has $lines lines, limit is $limit" >&2
      status=1
    fi
  done < <(find "$@" -maxdepth 1 -type f -name '*.rs' | sort)
}

check_shell_strict_mode() {
  local file
  for file in .githooks/pre-commit scripts/*.sh; do
    [[ -f "$file" ]] || continue
    if ! rg -q '^set -euo pipefail$' "$file"; then
      echo "shell script missing strict mode: $file" >&2
      status=1
    fi
  done
}

check_max_lines 320 src tests
check_shell_strict_mode

exit "$status"
