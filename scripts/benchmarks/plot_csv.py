#!/usr/bin/env python3
"""
Plot p95/p99 latency and RPS from wrk CSV files.

Requires matplotlib:
  python3 -m pip install matplotlib

Usage:
  scripts/benchmarks/plot_csv.py -o benchmarks/results/plot.png benchmarks/results/*.csv
"""
import argparse
import csv
from pathlib import Path
from typing import List, Dict, Any

import matplotlib.pyplot as plt


def load_csv(path: Path) -> Dict[str, Any]:
    with path.open() as f:
        reader = csv.DictReader(f)
        rows = list(reader)
        if not rows:
            raise ValueError(f"No rows in {path}")
        row = rows[0]
        row["source"] = str(path)
        return row


def main() -> None:
    parser = argparse.ArgumentParser(description="Plot wrk CSV metrics")
    parser.add_argument("files", nargs="+", help="CSV files")
    parser.add_argument("-o", "--output", required=True, help="Output PNG path")
    args = parser.parse_args()

    rows: List[Dict[str, Any]] = [load_csv(Path(p)) for p in args.files]
    labels = [r.get("label", "") for r in rows]
    p95 = [float(r.get("p95_ms", 0) or 0) for r in rows]
    p99 = [float(r.get("p99_ms", 0) or 0) for r in rows]
    rps = [float(r.get("rps", 0) or 0) for r in rows]

    fig, ax1 = plt.subplots(figsize=(8, 4))
    x = range(len(labels))

    ax1.bar([i - 0.15 for i in x], p95, width=0.3, label="p95 ms", color="#4e79a7")
    ax1.bar([i + 0.15 for i in x], p99, width=0.3, label="p99 ms", color="#f28e2b")
    ax1.set_ylabel("Latency (ms)")
    ax1.set_xticks(list(x))
    ax1.set_xticklabels(labels)

    ax2 = ax1.twinx()
    ax2.plot(list(x), rps, marker="o", color="#59a14f", label="RPS")
    ax2.set_ylabel("Requests/sec")

    lines, labels_left = ax1.get_legend_handles_labels()
    lines2, labels_right = ax2.get_legend_handles_labels()
    ax1.legend(lines + lines2, labels_left + labels_right, loc="upper right")

    plt.tight_layout()
    plt.savefig(args.output, dpi=150)
    print(f"saved {args.output}")


if __name__ == "__main__":
    main()
