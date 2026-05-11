#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
IMAGE="${IMAGE:-zkvm-page-cliff}"

if [[ -n "${CONTAINER_RUNTIME:-}" ]]; then
  CANDIDATES=("$CONTAINER_RUNTIME")
else
  CANDIDATES=("docker" "podman")
fi

RUNTIME=""
for candidate in "${CANDIDATES[@]}"; do
  if command -v "$candidate" >/dev/null 2>&1 && "$candidate" --version >/dev/null 2>&1; then
    RUNTIME="$candidate"
    break
  fi
done

if [[ -z "$RUNTIME" ]]; then
  echo "Install a working Docker or Podman runtime, then rerun this script." >&2
  exit 1
fi

"$RUNTIME" build -t "$IMAGE" "$ROOT_DIR"

mkdir -p "$ROOT_DIR/out"

VOLUME="$ROOT_DIR/out:/work/out"
if [[ "$RUNTIME" == *podman* ]]; then
  VOLUME="$VOLUME:Z"
fi

"$RUNTIME" run --rm -v "$VOLUME" "$IMAGE" "$@"
