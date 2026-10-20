#!/usr/bin/env bash
set -euo pipefail
: "${REGISTRY_OWNER:?Set the GHCR owner}"
: "${1:?Supply at least one image tag}"

for component in server plugins frontend; do
  case "$component" in
    server) image=ferret ;;
    plugins) image=ferret-plugins ;;
    frontend) image=ferret-frontend ;;
  esac
  archive="docker-archive:dist/${component}.tar.gz"
  test "$(skopeo inspect "$archive" | jq -r .Architecture)" = amd64
  for tag in "$@"; do
    skopeo copy --format oci "$archive" "docker://ghcr.io/${REGISTRY_OWNER}/${image}:${tag}"
  done
done
