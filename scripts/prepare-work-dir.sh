#!/usr/bin/env bash
# Validate and prepare the mkarchiso work directory.
# Deletion requires a sibling marker (${WORK_DIR}.02os-marker) so an arbitrary
# path is never removed. `/` is not a container: only exact critical paths and
# real overlaps with the repo, profile, output, home, or pacman cache are rejected.

set -euo pipefail

_prepare_work_dir_lib="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Return 0 when the two canonical paths are equal, one contains the other,
# or either is nested in the other. `/` does not count as a container.
_work_dir_overlaps() {
  local work="$1" other="$2" prefix
  [[ -z "${other}" ]] && return 1
  if [[ "${work}" == "${other}" ]]; then
    return 0
  fi
  if [[ "${work}" != "/" ]]; then
    prefix="${work}/"
    if [[ "${other:0:${#prefix}}" == "${prefix}" ]]; then
      return 0
    fi
  fi
  if [[ "${other}" != "/" ]]; then
    prefix="${other}/"
    if [[ "${work:0:${#prefix}}" == "${prefix}" ]]; then
      return 0
    fi
  fi
  return 1
}

_work_dir_assert_mutable() {
  local canonical="$1" home_canon="$2"
  if [[ -z "${canonical}" || "${canonical}" == "/" || "${canonical}" != /* ]]; then
    echo "ERROR: refusing to modify WORK_DIR (${canonical:-empty})." >&2
    return 1
  fi
  case "${canonical}" in
    /|/tmp|/var|/var/tmp|/home|/root|/usr|/etc|/boot|/opt|/var/cache)
      echo "ERROR: refusing to modify critical path ${canonical}." >&2
      return 1
      ;;
  esac
  if [[ -n "${home_canon}" && "${canonical}" == "${home_canon}" ]]; then
    echo "ERROR: refusing to modify HOME ${canonical}." >&2
    return 1
  fi
  return 0
}

_work_dir_unlock_owned_directories() {
  # mkarchiso leaves some copied directories mode 0555. Ownership alone does
  # not make their entries removable. Do not follow links or change foreign
  # directories; rm below still detects entries that require Docker reclaim.
  find -P "$1" -type d -uid "$2" -exec chmod u+w -- {} + 2>/dev/null || true
}

_work_dir_reclaim() {
  local canonical="$1" marker="$2" host_uid="$3" host_gid="$4" home_canon="$5"
  if [[ ! -f "${marker}" || -L "${marker}" ]]; then
    echo "ERROR: refusing to reclaim ${canonical} without a regular marker ${marker}." >&2
    return 1
  fi
  _work_dir_assert_mutable "${canonical}" "${home_canon}"
  echo "Work directory is not writable; reclaiming ownership inside Docker..."
  docker run --rm --privileged \
    -e HOST_UID="${host_uid}" \
    -e HOST_GID="${host_gid}" \
    -v "${canonical}:${canonical}" \
    archlinux:latest \
    chown -R "${host_uid}:${host_gid}" "${canonical}"
  if [[ ! -f "${marker}" || -L "${marker}" ]]; then
    echo "ERROR: marker disappeared; refusing to delete ${canonical}." >&2
    return 1
  fi
  _work_dir_assert_mutable "${canonical}" "${home_canon}"
  _work_dir_unlock_owned_directories "${canonical}" "${host_uid}"
  rm -rf -- "${canonical}"
}

prepare_work_dir() {
  local work_input="${1:-${WORK_DIR:-/var/tmp/02_OS-build-work}}"
  local repo_root="${SCRIPT_DIR:-}"
  local profile_dir="${PROFILE_DIR:-}"
  local out_dir="${OUT_DIR:-}"
  local cache_dir="${PACMAN_CACHE_DIR:-/var/cache/pacman/pkg}"
  local min_free_gb="${MIN_FREE_GB:-20}"
  local home_dir="${HOME:-}"
  local host_uid="${HOST_UID:-$(id -u)}"
  local host_gid="${HOST_GID:-$(id -g)}"
  local canonical="" home_canon="" df_path="" avail_kb="" min_kb="" marker=""
  local prot="" prot_canon="" denied=""

  if [[ -z "${repo_root}" ]]; then
    repo_root="$(cd "${_prepare_work_dir_lib}/.." && pwd)"
  fi
  if [[ -z "${profile_dir}" ]]; then
    profile_dir="${repo_root}/profile"
  fi
  if [[ -z "${out_dir}" ]]; then
    out_dir="${repo_root}/out"
  fi

  if [[ -z "${work_input}" ]]; then
    echo "ERROR: WORK_DIR is empty." >&2
    return 1
  fi

  # readlink -f resolves a missing final component when the parent exists.
  # Failure means a parent is missing; do not mkdir it.
  if ! canonical="$(readlink -f -- "${work_input}")"; then
    echo "ERROR: WORK_DIR parent must already exist (not creating parents): ${work_input}" >&2
    return 1
  fi
  if [[ -z "${canonical}" || "${canonical}" != /* ]]; then
    echo "ERROR: refusing non-absolute WORK_DIR: ${work_input}" >&2
    return 1
  fi

  if [[ -n "${home_dir}" ]]; then
    home_canon="$(readlink -f -- "${home_dir}" 2>/dev/null || true)"
  fi

  # Exact critical paths only. Children such as /var/tmp/02_OS-build-work are allowed.
  local -a exact_denied=(
    / /tmp /var /var/tmp /home /root /usr /etc /boot /opt /var/cache
  )
  for denied in "${exact_denied[@]}"; do
    if [[ "${canonical}" == "${denied}" ]]; then
      echo "ERROR: WORK_DIR ${canonical} is a critical path and cannot be used." >&2
      return 1
    fi
  done
  if [[ -n "${home_canon}" && "${canonical}" == "${home_canon}" ]]; then
    echo "ERROR: WORK_DIR ${canonical} is HOME and cannot be used." >&2
    return 1
  fi

  local -a protected=("${repo_root}" "${profile_dir}" "${out_dir}" /root "${cache_dir}")
  if [[ -n "${home_dir}" ]]; then
    protected+=("${home_dir}")
  fi
  for prot in "${protected[@]}"; do
    [[ -z "${prot}" ]] && continue
    if ! prot_canon="$(readlink -f -- "${prot}" 2>/dev/null)"; then
      continue
    fi
    if _work_dir_overlaps "${canonical}" "${prot_canon}"; then
      echo "ERROR: WORK_DIR ${canonical} overlaps protected path ${prot_canon}." >&2
      return 1
    fi
  done

  df_path="${canonical}"
  if [[ ! -d "${df_path}" ]]; then
    df_path="$(dirname -- "${canonical}")"
  fi
  if [[ ! -d "${df_path}" ]]; then
    echo "ERROR: cannot check free space for ${canonical}; parent is missing." >&2
    return 1
  fi
  avail_kb="$(df -Pk "${df_path}" | awk 'NR==2 {print $4}')"
  min_kb=$((min_free_gb * 1024 * 1024))
  if [[ -z "${avail_kb}" || "${avail_kb}" -lt "${min_kb}" ]]; then
    echo "ERROR: ${canonical} needs at least ${min_free_gb} GB free on ${df_path} (available KB: ${avail_kb:-unknown})." >&2
    echo "Set WORK_DIR to a filesystem with enough space and retry." >&2
    return 1
  fi

  # Validation is finished. Nothing above creates, deletes, or chowns.
  _work_dir_assert_mutable "${canonical}" "${home_canon}"

  # The lock lives outside the reclaimed tree and is shared by all checkouts.
  # Keep fd 8 open for the caller's entire build, not just directory preparation.
  local work_lock="${canonical}.02os-lock"
  if [[ -L "${work_lock}" || ( -e "${work_lock}" && ! -f "${work_lock}" ) ]]; then
    echo "ERROR: refusing non-regular work-directory lock ${work_lock}." >&2
    return 1
  fi
  exec 8>>"${work_lock}"
  if ! flock -n 8; then
    echo "ERROR: another build owns work directory ${canonical}." >&2
    return 1
  fi

  marker="${canonical}.02os-marker"
  if [[ -L "${marker}" ]]; then
    echo "ERROR: refusing symlink marker ${marker}." >&2
    return 1
  fi
  if [[ -e "${canonical}" || -L "${canonical}" ]]; then
    if [[ ! -f "${marker}" ]]; then
      echo "ERROR: ${canonical} exists without marker ${marker}; refusing to delete." >&2
      return 1
    fi
    _work_dir_assert_mutable "${canonical}" "${home_canon}"
    _work_dir_unlock_owned_directories "${canonical}" "${host_uid}"
    if ! rm -rf -- "${canonical}"; then
      _work_dir_reclaim "${canonical}" "${marker}" "${host_uid}" "${host_gid}" "${home_canon}"
    fi
    if [[ -e "${canonical}" || -L "${canonical}" ]]; then
      echo "ERROR: failed to remove marked work directory ${canonical}." >&2
      return 1
    fi
  fi

  printf '02os-work-dir\n%s\n' "${canonical}" > "${marker}"
  mkdir -- "${canonical}"
  WORK_DIR="${canonical}"
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
  prepare_work_dir "$@"
fi
