"""Report medians and paired differences from a Linux echo comparison."""
from collections import defaultdict
from pathlib import Path
from statistics import median
import argparse
import re


def read_metric(path, key):
    for line in path.read_text().splitlines():
        fields = line.split()
        if fields and fields[0] == key:
            return float(fields[1])
    raise ValueError(f"{path}: missing {key}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--reference", default="tokio")
    args = parser.parse_args()
    samples = defaultdict(dict)
    for path in args.directory.glob("*.txt"):
        match = re.fullmatch(r"(\d+)-(\d+)-([a-z0-9]+)\.txt", path.name)
        if not match:
            continue
        size, round_number, name = match.groups()
        cpu = path.with_name(f"{path.stem}.cpu.txt")
        sample = (
            read_metric(path, "qps"),
            read_metric(cpu, "server_cpu_us_per_request"),
            read_metric(path, "p99"),
        )
        if min(sample) <= 0:
            raise ValueError(f"{path}: invalid zero or negative metric")
        samples[int(size), name][int(round_number)] = sample

    if not samples:
        raise ValueError("no benchmark samples")
    print(f"Paired changes relative to **{args.reference}**. Latency is closed-loop.")
    print()
    print("| Bytes | Variant | Rounds | RPS | CPU µs/request | p99 µs | Δ RPS | Δ CPU | Δ p99 |")
    print("|---:|---|---:|---:|---:|---:|---:|---:|---:|")
    for (size, name), rounds in sorted(samples.items()):
        ref = samples[size, args.reference]
        if set(rounds) != set(ref):
            raise ValueError(f"{size}/{name}: rounds do not match {args.reference}")
        values = [median(metric) for metric in zip(*rounds.values())]
        changes = [
            median(rounds[r][i] / ref[r][i] - 1 for r in rounds)
            for i in range(len(values))
        ]
        rps, cpu, p99 = values
        throughput, cpu_delta, p99_delta = changes
        print(
            f"| {size} | {name} | {len(rounds)} | {rps:,.0f} | {cpu:.2f} | {p99:.2f} | "
            f"{throughput:+.1%} | {cpu_delta:+.1%} | {p99_delta:+.1%} |"
        )


if __name__ == "__main__":
    main()
