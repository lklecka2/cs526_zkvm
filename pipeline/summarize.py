#!/usr/bin/env python3
import json
import pathlib
import sys


def load_json(path: pathlib.Path) -> dict:
    if not path.exists():
        return {}
    return json.loads(path.read_text(encoding="utf-8"))


def main() -> None:
    if len(sys.argv) != 2:
        raise SystemExit("usage: summarize.py <out-dir>")

    out_dir = pathlib.Path(sys.argv[1])
    execute = load_json(out_dir / "zkvm" / "execute.json")
    metrics_dir = out_dir / "metrics"
    metrics_dir.mkdir(parents=True, exist_ok=True)

    user_cycles = execute.get("user_cycles") or 0
    paging_cycles = execute.get("paging_cycles") or 0
    total_cycles = execute.get("total_cycles") or 0
    dynamic_instruction_count = execute.get("dynamic_instruction_count") or 0

    summary = {
        "benchmark": execute.get("benchmark"),
        "profile": execute.get("profile"),
        "size": execute.get("size"),
        "committed_i32": execute.get("committed_i32"),
        "elf_sha256": execute.get("elf_sha256"),
        "executor_wall_ms": execute.get("executor_wall_ms"),
        "user_cycles": user_cycles,
        "paging_cycles": paging_cycles,
        "reserved_cycles": execute.get("reserved_cycles"),
        "total_cycles": total_cycles,
        "segment_count": execute.get("segment_count"),
        "segment_po2s": execute.get("segment_po2s"),
        "dynamic_instruction_count": dynamic_instruction_count,
        "unique_pcs_executed": execute.get("unique_pcs_executed"),
        "page_in_count": execute.get("page_in_count"),
        "page_out_count": execute.get("page_out_count"),
        "unique_pages_written": execute.get("unique_pages_written"),
        "source_paging_enabled": execute.get("source_paging_enabled", False),
        "synthetic_page_in_count": execute.get("synthetic_page_in_count"),
        "synthetic_page_out_count": execute.get("synthetic_page_out_count"),
        "source_page_hotspots_path": execute.get("source_page_hotspots_path"),
        "segment_model": execute.get("segment_model"),
        "paging_overhead_ratio": round(paging_cycles / user_cycles, 6) if user_cycles else None,
        "cycles_per_dynamic_instruction": round(total_cycles / dynamic_instruction_count, 6)
        if dynamic_instruction_count
        else None,
    }

    (metrics_dir / "summary.json").write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n",
        encoding="utf-8",
    )
    print(json.dumps(summary, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
