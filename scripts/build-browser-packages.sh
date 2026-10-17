#!/usr/bin/env bash
# Resolve fresh signed stable metadata, then build local native pacman packages.
set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ ${1:-} != --inside-container ]]; then
  output="${REPO_ROOT}/.cache/browser-repo"
  resolve_only=0
  while [[ $# -gt 0 ]]; do
    case "$1" in
      --output) [[ $# -ge 2 ]] || exit 2; output="$2"; shift 2 ;;
      --resolve-only) resolve_only=1; shift ;;
      *) echo "Usage: $0 [--output DIR] [--resolve-only]" >&2; exit 2 ;;
    esac
  done
  mkdir -p -- "${output}"
  output="$(realpath -- "${output}")"
  case "${output}" in /|/home|/tmp|"${REPO_ROOT}"|"${REPO_ROOT}/profile")
    echo 'Refusing an unsafe browser repository output directory' >&2; exit 2 ;;
  esac
  browser_cache="${REPO_ROOT}/.cache/browser-build-pacman"
  mkdir -p "${browser_cache}"
  exec 9>"${output}.lock"
  flock -n 9 || { echo 'Another browser package build is active' >&2; exit 1; }
  docker run --rm \
    -e "HOST_UID=$(id -u)" -e "HOST_GID=$(id -g)" \
    -e "RESOLVE_ONLY=${resolve_only}" \
    --mount "type=bind,src=${REPO_ROOT},dst=/source,readonly" \
    --mount "type=bind,src=${output},dst=/browser-repo" \
    --mount "type=bind,src=${browser_cache},dst=/var/cache/pacman/pkg" \
    "${BROWSER_BUILD_IMAGE:-archlinux:latest}" \
    bash /source/scripts/build-browser-packages.sh --inside-container
  exit
fi
shift
[[ $# == 0 && $(id -u) == 0 && -d /browser-repo ]]
pacman-key --init
pacman -Syu --needed --noconfirm base-devel python gnupg curl
useradd --create-home --shell /bin/bash browserbuilder
work="$(mktemp -d /tmp/02os-browser-build.XXXXXXXX)"
trap 'rm -rf -- "${work}"' EXIT
chmod 755 "${work}"
mkdir "${work}/sources" "${work}/packages" "${work}/repo"
chown -R browserbuilder:browserbuilder "${work}"
runuser -u browserbuilder -- python /source/scripts/browser-packages/resolve_sources.py \
  --output "${work}/sources"
if [[ ${RESOLVE_ONLY:-0} == 1 ]]; then
  cp "${work}/sources/sources.json" /browser-repo/resolved-sources.json
  chown "${HOST_UID}:${HOST_GID}" /browser-repo/resolved-sources.json
  exit
fi
for browser in google-chrome brave-bin; do
  build="${work}/packages/${browser}"
  mkdir "${build}"
  cp "/source/scripts/browser-packages/${browser}/PKGBUILD" "${build}/PKGBUILD"
  cp "${work}/sources/${browser}/browser.deb" "${build}/browser.deb"
  python - "${build}/PKGBUILD" "${work}/sources/${browser}/source.json" <<'PY'
import json,re,sys
from pathlib import Path
path=Path(sys.argv[1]);source=json.loads(Path(sys.argv[2]).read_text())
text=path.read_text()
text,count=re.subn(r'^pkgver=0$', 'pkgver='+source['pkgver'], text, flags=re.M)
assert count==1
text,count=re.subn(r'^pkgrel=1$', 'pkgrel='+source.get('pkgrel','1'), text, flags=re.M)
assert count==1
text=text.replace('REPLACED_BY_VERIFIED_MANIFEST',source['source_sha256'])
path.write_text(text)
PY
  chown -R browserbuilder:browserbuilder "${build}"
  # Dependencies are represented in package metadata, and installed by pacman
  # in the image/target. Repackaging a vendor binary needs none of those at build.
  runuser -u browserbuilder -- bash -c 'cd "$1"; makepkg --nodeps --noconfirm --cleanbuild' _ "${build}"
  packages=("${build}"/*.pkg.tar.zst)
  [[ ${#packages[@]} == 1 && -f ${packages[0]} ]]
  cp "${packages[0]}" "${work}/repo/"
done
repo-add "${work}/repo/02os-browsers.db.tar.gz" "${work}/repo/"*.pkg.tar.zst
# Use regular database files so image/installer path validation need not trust links.
rm "${work}/repo/02os-browsers.db" "${work}/repo/02os-browsers.files"
cp "${work}/repo/02os-browsers.db.tar.gz" "${work}/repo/02os-browsers.db"
cp "${work}/repo/02os-browsers.files.tar.gz" "${work}/repo/02os-browsers.files"
python - "${work}/repo" "${work}/sources/sources.json" <<'PY'
from datetime import datetime,timezone
import hashlib,json,subprocess,sys
from pathlib import Path
root=Path(sys.argv[1]);sources=json.loads(Path(sys.argv[2]).read_text())
def sha(path): return hashlib.sha256(path.read_bytes()).hexdigest()
packages=[]
for path in sorted(root.glob('*.pkg.tar.zst')):
 info=subprocess.check_output(['pacman','-Qp',str(path)],text=True).split()
 packages.append({'name':info[0],'version':info[1],'filename':path.name,'sha256':sha(path)})
assert {p['name'] for p in packages}=={'google-chrome','brave-bin'}
archive=root/'02os-browsers.db.tar.gz';database=root/'02os-browsers.db'
manifest={'format_version':1,'architecture':'x86_64','generated_at_utc':datetime.now(timezone.utc).isoformat(),
 'packages':packages,'repository':{'name':'02os-browsers','filename':archive.name,'sha256':sha(archive),
 'database_filename':database.name,'database_sha256':sha(database)},'vendor_sources':sources}
(root/'manifest.json').write_text(json.dumps(manifest,indent=2)+'\n')
PY
# Publish the manifest last. Callers use its exact archive names and hashes.
cp "${work}/repo/"*.pkg.tar.zst "${work}/repo/"*.gz \
   "${work}/repo/02os-browsers.db" "${work}/repo/02os-browsers.files" /browser-repo/
cp "${work}/repo/manifest.json" /browser-repo/manifest.json
chown -R "${HOST_UID}:${HOST_GID}" /browser-repo
cat /browser-repo/manifest.json
