# CS 526 Final Project Code Artifacts

This repo contains the relevant code to produce the data and figures in the final report.

It contains:

- `benchmarks/page_boundary_aligned_padded`: page-aligned hot working set
- `benchmarks/page_boundary_malloc_split`: default allocator, boundary-crossing hot working set
- `crates/guest`: RISC Zero guest wrapper for C benchmarks
- `crates/host`: RISC Zero executor and source-paging measurement backend
- `pipeline/page_boundary_cliff.py`: runs both cases and writes a summary
- `artifacts/recursion_zkr.zip`: pinned RISC Zero artifact used by `risc0-circuit-recursion`

## Run With A Container

From a fresh clone, the container path is the intended setup. It installs Rust,
RISC Zero, LLVM, and Python inside the image.

```bash
cd prod
bash run-container.sh
```

The script uses Docker if available, otherwise Podman. To force one:

```bash
CONTAINER_RUNTIME=podman bash run-container.sh
CONTAINER_RUNTIME=docker bash run-container.sh
```

Arguments after the script are passed to the experiment driver:

```bash
bash run-container.sh --sizes tiny --segment-limit-po2 16
```

Manual Docker commands:

```bash
docker build -t zkvm-page-cliff .
docker run --rm -v "$PWD/out:/work/out" zkvm-page-cliff
```

Manual Podman commands:

```bash
podman build -t zkvm-page-cliff .
podman run --rm -v "$PWD/out:/work/out:Z" zkvm-page-cliff
```

The main outputs are:

```text
out/experiments/page_boundary_cliff/summary.md
out/experiments/page_boundary_cliff/summary.json
```

## Run Locally

Install prerequisites:

- Rust toolchain
- RISC Zero `risc0` Rust toolchain
- `clang`, `opt`, `llc`, `llvm-ar`
- Python 3

Then run:

```bash
./pipeline/page_boundary_cliff.py --profile baseline --sizes large --segment-limit-po2 16
```

## Expected Result

The `malloc-split` case should have substantially higher paging cycles and synthetic page events than the aligned case, while user cycles remain close.

The exact numbers vary by RISC Zero version and host setup. In the original run:

```text
aligned:      76,334,338 paging cycles
malloc-split: 136,818,990 paging cycles
```

## Files Worth Reading

```text
benchmarks/page_boundary_aligned_padded/main.c
benchmarks/page_boundary_malloc_split/main.c
crates/host/src/main.rs
pipeline/page_boundary_cliff.py
```
