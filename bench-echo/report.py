#!/usr/bin/env python3
"""Turn raw rounds into a statement that says only what the data supports.

Each round measures both servers seconds apart, so a per-round delta cancels
whatever the machine happened to be doing. What matters is whether those
deltas agree with each other. If they change sign, the run has not found a
difference, and quoting the median would be inventing one.

That is not hypothetical: an earlier version of this comparison reported
-2.4% from three rounds. More rounds gave +3.4%, -4.8%, -11.8% and +5.6%.
"""
import sys


def median(xs):
    xs = sorted(xs)
    n = len(xs)
    return xs[n // 2] if n % 2 else (xs[n // 2 - 1] + xs[n // 2]) / 2


def verdict(deltas, name, lower_is_better=False):
    lo, hi, mid = min(deltas), max(deltas), median(deltas)
    print(f"  {name:<12} median {mid:+6.1f}%   range {lo:+.1f}% .. {hi:+.1f}%")
    if lo < 0 < hi:
        return f"  {name}: no difference established, the deltas change sign."
    floor = min(abs(lo), abs(hi))
    better = "geario ahead" if (mid < 0) == lower_is_better else "geario behind"
    return f"  {name}: every round agrees. {better} by at least {floor:.1f}%."


def block(rows, header):
    gq = [r[0] for r in rows]
    nq = [r[1] for r in rows]
    gp = [r[2] for r in rows]
    np_ = [r[3] for r in rows]

    print(f"\n{header}   ({len(rows)} rounds)")
    print(f"  qps   geario {median(gq):>9,.0f}   ntex {median(nq):>9,.0f}")
    print(f"  p50   geario {median(gp):>9.1f}us  ntex {median(np_):>9.1f}us")
    print()

    notes = [
        verdict([(a - b) / b * 100 for a, b in zip(gq, nq)], "throughput"),
        verdict([(a - b) / b * 100 for a, b in zip(gp, np_)], "latency", True),
    ]
    print()
    for n in notes:
        print(n)


def main() -> int:
    blocks, header, rows = [], None, []
    for line in sys.stdin:
        line = line.strip()
        if not line:
            continue
        if line.startswith("#"):
            if rows:
                blocks.append((header, rows))
            header, rows = line.lstrip("# "), []
            continue
        rows.append([float(x) for x in line.split()])
    if rows:
        blocks.append((header, rows))

    if not blocks:
        print("no data")
        return 1
    for header, rows in blocks:
        if len(rows) < 4:
            print(f"\n{header}: only {len(rows)} rounds, not enough to say anything")
            continue
        block(rows, header)
    return 0


if __name__ == "__main__":
    sys.exit(main())
