#!/usr/bin/env bash
# Print the checkout for one sibling component.
# Order: explicit directory variable, vendor submodule, sibling directory.
resolve_one() {
  local env_name="$1"
  local marker="$2"
  local vendor_rel="$3"
  local sibling="$4"
  local script_dir repo_root candidate value
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  repo_root="$(cd "${script_dir}/.." && pwd)"

  value="${!env_name:-}"
  if [[ -n "${value}" && -e "${value}/${marker}" ]]; then
    (cd "${value}" && pwd)
    return 0
  fi

  candidate="${repo_root}/${vendor_rel}"
  if [[ -e "${candidate}/${marker}" ]]; then
    (cd "${candidate}" && pwd)
    return 0
  fi

  candidate="${repo_root}/../${sibling}"
  if [[ -e "${candidate}/${marker}" ]]; then
    (cd "${candidate}" && pwd)
    return 0
  fi

  echo "ERROR: cannot find ${sibling}. Set ${env_name}, or init the ${vendor_rel} submodule, or clone it next to this repo." >&2
  return 1
}
