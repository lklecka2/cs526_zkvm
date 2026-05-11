#!/usr/bin/env python3
import argparse
import json
import pathlib
import subprocess
import sys


ROOT = pathlib.Path(__file__).resolve().parents[1]
EXPERIMENT_DIR = ROOT / "out" / "experiments" / "page_boundary_cliff"
BENCHMARKS = ["page_boundary_malloc_split", "page_boundary_aligned_padded"]


def load_json(path: pathlib.Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def run_case(benchmark: str, profile: str, size: str, segment_limit_po2: int, no_cache: bool) -> dict:
    cmd = [
        "bash",
        str(ROOT / "pipeline" / "run.sh"),
        benchmark,
        "--profile",
        profile,
        "--size",
        size,
        "--source-paging",
        "--segment-limit-po2",
        str(segment_limit_po2),
    ]
    if no_cache:
        cmd.append("--no-cache")
    subprocess.run(cmd, cwd=ROOT, check=True)

    out_dir = ROOT / "out" / benchmark / profile / size
    summary = load_json(out_dir / "metrics" / "summary.json")
    execute = load_json(out_dir / "zkvm" / "execute.json")
    source_summary = load_json(out_dir / "analysis" / "source-pages-summary.json")
    return {
        "benchmark": benchmark,
        "profile": profile,
        "size": size,
        "segment_limit_po2": segment_limit_po2,
        "summary_path": str(out_dir / "metrics" / "summary.json"),
        "source_hotspots_path": str(out_dir / "analysis" / "source-page-hotspots.md"),
        "committed_i32": summary.get("committed_i32"),
        "valid": summary.get("committed_i32") not in (-1, -2, -3),
        "user_cycles": summary.get("user_cycles"),
        "paging_cycles": summary.get("paging_cycles"),
        "total_cycles": summary.get("total_cycles"),
        "dynamic_instruction_count": summary.get("dynamic_instruction_count"),
        "segment_count": summary.get("segment_count"),
        "unique_pages_written": summary.get("unique_pages_written"),
        "synthetic_page_in_count": summary.get("synthetic_page_in_count"),
        "synthetic_page_out_count": summary.get("synthetic_page_out_count"),
        "paging_overhead_ratio": summary.get("paging_overhead_ratio"),
        "executor_wall_ms": summary.get("executor_wall_ms"),
        "segment_model": summary.get("segment_model"),
        "elf_sha256": summary.get("elf_sha256"),
        "reserved_cycles": execute.get("reserved_cycles"),
        "top_hotspots": source_summary.get("top_hotspots", [])[:5],
    }


def ratio(numerator, denominator):
    if numerator is None or denominator in (None, 0):
        return None
    return round(numerator / denominator, 6)


def build_comparison(cases: list[dict]) -> list[dict]:
    by_size = {}
    for case in cases:
        by_size.setdefault(case["size"], {})[case["benchmark"]] = case

    comparisons = []
    for size, group in sorted(by_size.items()):
        split = group.get("page_boundary_malloc_split")
        aligned = group.get("page_boundary_aligned_padded")
        if not split or not aligned:
            continue
        comparisons.append(
            {
                "size": size,
                "malloc_valid": split["valid"],
                "aligned_valid": aligned["valid"],
                "paging_cycles_ratio": ratio(split["paging_cycles"], aligned["paging_cycles"]),
                "total_cycles_ratio": ratio(split["total_cycles"], aligned["total_cycles"]),
                "synthetic_page_in_ratio": ratio(
                    split["synthetic_page_in_count"], aligned["synthetic_page_in_count"]
                ),
                "unique_pages_written_delta": (
                    split["unique_pages_written"] - aligned["unique_pages_written"]
                    if split["unique_pages_written"] is not None
                    and aligned["unique_pages_written"] is not None
                    else None
                ),
            }
        )
    return comparisons


def render_markdown(cases: list[dict], comparisons: list[dict]) -> str:
    lines = [
        "# Page Boundary Cliff Experiment",
        "",
        "The malloc case uses the default guest allocator and validates that hot 1024-byte groups straddle page boundaries. The aligned case pads each group to a 2048-byte page-aligned slot while touching only the first 1024 bytes.",
        "",
        "## Cases",
        "",
        "| benchmark | size | valid | segments | paging cycles | total cycles | user cycles | synthetic ins | synthetic outs | unique dirty pages |",
        "| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |",
    ]
    for case in cases:
        lines.append(
            "| {benchmark} | {size} | {valid} | {segment_count} | {paging_cycles} | {total_cycles} | {user_cycles} | {synthetic_page_in_count} | {synthetic_page_out_count} | {unique_pages_written} |".format(
                **case
            )
        )

    lines.extend(
        [
            "",
            "## Ratios",
            "",
            "| size | malloc valid | aligned valid | paging cycles ratio | total cycles ratio | synthetic page-in ratio | dirty page delta |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: |",
        ]
    )
    for comp in comparisons:
        lines.append(
            "| {size} | {malloc_valid} | {aligned_valid} | {paging_cycles_ratio} | {total_cycles_ratio} | {synthetic_page_in_ratio} | {unique_pages_written_delta} |".format(
                **comp
            )
        )

    lines.extend(["", "## Hotspot Files", ""])
    for case in cases:
        lines.append(
            "- `{benchmark}` `{size}`: `{source_hotspots_path}`".format(**case)
        )
    lines.append("")
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Run the page-boundary cliff experiment.")
    parser.add_argument("--profile", default="baseline")
    parser.add_argument("--sizes", default="large,xlarge")
    parser.add_argument("--segment-limit-po2", type=int, default=16)
    parser.add_argument("--no-cache", action="store_true")
    args = parser.parse_args()

    sizes = [size.strip() for size in args.sizes.split(",") if size.strip()]
    if not sizes:
        raise SystemExit("at least one size is required")

    cases = []
    for size in sizes:
        for benchmark in BENCHMARKS:
            cases.append(
                run_case(
                    benchmark,
                    args.profile,
                    size,
                    args.segment_limit_po2,
                    args.no_cache,
                )
            )

    comparisons = build_comparison(cases)
    EXPERIMENT_DIR.mkdir(parents=True, exist_ok=True)
    output = {
        "profile": args.profile,
        "sizes": sizes,
        "segment_limit_po2": args.segment_limit_po2,
        "cases": cases,
        "comparisons": comparisons,
    }
    (EXPERIMENT_DIR / "summary.json").write_text(
        json.dumps(output, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    (EXPERIMENT_DIR / "summary.md").write_text(
        render_markdown(cases, comparisons), encoding="utf-8"
    )
    print(json.dumps(output, indent=2, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as err:
        print(f"command failed with exit status {err.returncode}: {err.cmd}", file=sys.stderr)
        raise
