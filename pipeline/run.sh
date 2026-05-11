#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  ./pipeline/run.sh <benchmark> [--profile baseline] [--size tiny|small|medium|large|xlarge|huge] [--source-paging] [--segment-limit-po2 N] [--no-cache]

Benchmarks:
  page_boundary_aligned_padded
  page_boundary_malloc_split

Example:
  ./pipeline/run.sh page_boundary_aligned_padded --size large --source-paging --segment-limit-po2 16
EOF
}

if [[ $# -lt 1 ]]; then
  usage
  exit 1
fi

BENCHMARK="$1"
shift

PROFILE="baseline"
SIZE="small"
NO_CACHE=0
SOURCE_PAGING=0
SEGMENT_LIMIT_PO2=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --profile)
      PROFILE="$2"
      shift 2
      ;;
    --size)
      SIZE="$2"
      shift 2
      ;;
    --no-cache)
      NO_CACHE=1
      shift
      ;;
    --source-paging)
      SOURCE_PAGING=1
      shift
      ;;
    --segment-limit-po2)
      SEGMENT_LIMIT_PO2="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage
      exit 1
      ;;
  esac
done

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BENCH_DIR="$ROOT_DIR/benchmarks/$BENCHMARK"
PROFILE_FILE="$ROOT_DIR/configs/profiles/$PROFILE.env"
OUT_DIR="$ROOT_DIR/out/$BENCHMARK/$PROFILE/$SIZE"
BUILD_DIR="$OUT_DIR/build"
METRICS_DIR="$OUT_DIR/metrics"
ANALYSIS_DIR="$OUT_DIR/analysis"

if [[ ! -f "$BENCH_DIR/main.c" ]]; then
  echo "Benchmark not found: $BENCHMARK" >&2
  find "$ROOT_DIR/benchmarks" -mindepth 1 -maxdepth 1 -type d -printf '  %f\n' | sort >&2
  exit 1
fi
if [[ ! -f "$PROFILE_FILE" ]]; then
  echo "Profile not found: $PROFILE" >&2
  exit 1
fi

mkdir -p "$BUILD_DIR" "$METRICS_DIR" "$ANALYSIS_DIR"

# shellcheck disable=SC1090
source "$PROFILE_FILE"

export ZKVM_C_BENCH="$BENCHMARK"
export ZKVM_BENCH_SIZE="$SIZE"
export ZK_CFLAGS="${ZK_CFLAGS:-"-O0"}"
export ZK_PASSES="${ZK_PASSES:-"lower-atomic"}"
export ZK_LLVMFLAGS="${ZK_LLVMFLAGS:-""}"
export ZK_CLANG_PATH="${ZK_CLANG_PATH:-clang}"
export ZK_OPT_PATH="${ZK_OPT_PATH:-opt}"
export ZK_LLC_PATH="${ZK_LLC_PATH:-llc}"
export ZK_LOOP_RISK=0
export ZK_LOOP_RISK_PLUGIN_HASH=""
export ZK_SOURCE_PAGING="$SOURCE_PAGING"
export RECURSION_SRC_PATH="${RECURSION_SRC_PATH:-"$ROOT_DIR/artifacts/recursion_zkr.zip"}"
if [[ "$SOURCE_PAGING" == "1" && -z "$SEGMENT_LIMIT_PO2" ]]; then
  SEGMENT_LIMIT_PO2=16
fi

GUEST_TARGET_DIR="$BUILD_DIR/target"
GUEST_PACKAGE="zkvm-c-guest"
GUEST_BINARY="zkvm-c-guest"
ELF="$BUILD_DIR/$GUEST_BINARY"
MANIFEST="$BUILD_DIR/build_manifest.txt"

SOURCE_HASH="$(
  {
    sha256sum "$BENCH_DIR/main.c"
    sha256sum "$ROOT_DIR/include/zkvm.h"
    sha256sum "$ROOT_DIR/include/zkvm_arena.h"
    sha256sum "$ROOT_DIR/crates/guest/build.rs"
    sha256sum "$ROOT_DIR/crates/guest/src/main.rs"
    sha256sum "$ROOT_DIR/crates/host/src/main.rs"
    sha256sum "$RECURSION_SRC_PATH"
    printf '%s\n' "$BENCHMARK" "$PROFILE" "$SIZE" "$ZK_CFLAGS" "$ZK_PASSES" "$ZK_LLVMFLAGS" "$ZK_CLANG_PATH" "$ZK_OPT_PATH" "$ZK_LLC_PATH" "$ZK_SOURCE_PAGING" "$SEGMENT_LIMIT_PO2" "$RECURSION_SRC_PATH"
  } | sha256sum | cut -d' ' -f1
)"

if [[ "$NO_CACHE" -eq 0 && -f "$MANIFEST" && -f "$ELF" ]] && grep -q "^source_hash=$SOURCE_HASH$" "$MANIFEST"; then
  echo "compile cache hit: $BENCHMARK $PROFILE $SIZE"
else
  CARGO_TARGET_DIR="$GUEST_TARGET_DIR" cargo +risc0 build -p "$GUEST_PACKAGE" --release --target riscv32im-risc0-zkvm-elf
  cp "$GUEST_TARGET_DIR/riscv32im-risc0-zkvm-elf/release/$GUEST_BINARY" "$ELF"
  sha256sum "$ELF" > "$BUILD_DIR/$GUEST_BINARY.sha256"
  cat > "$MANIFEST" <<EOF_MANIFEST
benchmark=$BENCHMARK
profile=$PROFILE
size=$SIZE
source_hash=$SOURCE_HASH
zk_cflags=$ZK_CFLAGS
zk_passes=$ZK_PASSES
zk_llvmflags=$ZK_LLVMFLAGS
clang=$ZK_CLANG_PATH
opt=$ZK_OPT_PATH
llc=$ZK_LLC_PATH
zk_source_paging=$ZK_SOURCE_PAGING
segment_limit_po2=$SEGMENT_LIMIT_PO2
recursion_src_path=$RECURSION_SRC_PATH
EOF_MANIFEST
fi

HOST_ARGS=(
  --benchmark "$BENCHMARK"
  --profile "$PROFILE"
  --size "$SIZE"
  --elf "$ELF"
  --out-dir "$OUT_DIR/zkvm"
  --analysis-dir "$ANALYSIS_DIR"
)
if [[ "$SOURCE_PAGING" == "1" ]]; then
  HOST_ARGS+=(--source-paging)
fi
if [[ -n "$SEGMENT_LIMIT_PO2" ]]; then
  HOST_ARGS+=(--segment-limit-po2 "$SEGMENT_LIMIT_PO2")
fi

env -u ZKVM_C_BENCH \
  -u ZKVM_BENCH_SIZE \
  -u ZK_CFLAGS \
  -u ZK_PASSES \
  -u ZK_LLVMFLAGS \
  -u ZK_CLANG_PATH \
  -u ZK_OPT_PATH \
  -u ZK_LLC_PATH \
  -u ZK_LOOP_RISK \
  -u ZK_LOOP_RISK_PLUGIN \
  -u ZK_LOOP_RISK_PLUGIN_HASH \
  -u ZK_SOURCE_PAGING \
  RECURSION_SRC_PATH="$RECURSION_SRC_PATH" \
  cargo run -p zkvm-c-host -- "${HOST_ARGS[@]}"

python3 "$ROOT_DIR/pipeline/summarize.py" "$OUT_DIR"
echo "Artifacts written to $OUT_DIR"
