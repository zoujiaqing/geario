#!/usr/bin/env python3
"""Focused perf acceptance: paired A/B per (driver, body).

A = geario-before, B = geario-after. Verdicts are pre-registered; a CI merely
crossing 1.0 is never a pass, and no metric is called "parity" unless its CI
actually excludes a >3% move.

  qps (higher better): PASS only if 95% CI lower bound of mean(B/A) > 0.97.
  p99 / cpu-per-req (lower better):
      improved         if CI upper  < 1.00
      non-inferior     if CI upper <= 1.03
      REGRESSED (>3%)  if CI lower  > 1.03
      NOT ESTABLISHED  otherwise (CI admits a >3% increase)
  Any round with bad>0 makes the whole config INVALID (no verdict).
Usage: analyze.py <raw.txt> [...]
"""
import re, sys, random, statistics
random.seed(11)
ROW = re.compile(r'^(\d+)\s+([AB])\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+([\d.]+)\s+(\d+)')
def ci(rs):
    b = sorted(sum(random.choice(rs) for _ in rs) / len(rs) for _ in range(10000))
    return b[250], b[9750]
def lower_better(lo, hi):
    if hi < 1.0:  return "improved"
    if hi <= 1.03: return "non-inferior (<=3%)"
    if lo > 1.03: return "REGRESSED (>3%)"
    return "NOT ESTABLISHED"
def run(path):
    blocks = re.split(r'#{3,}\s*(.+? body=\d+)\s*#{3,}', open(path).read())
    for i in range(1, len(blocks), 2):
        label = blocks[i]; rows = {}
        for line in blocks[i + 1].splitlines():
            m = ROW.match(line.strip())
            if m:
                rows.setdefault(int(m.group(1)), {})[m.group(2)] = [float(m.group(3 + k)) for k in range(6)]
        pairs = [(v['A'], v['B']) for v in rows.values()
                 if 'A' in v and 'B' in v and v['A'][0] > 0 and v['B'][0] > 0]
        if not pairs:
            continue
        bad = sum(a[5] + b[5] for a, b in pairs)
        tag = "  [INVALID: bad>0]" if bad > 0 else ""
        print(f"=== {label}: {len(pairs)} paired rounds, bad={bad:.0f}{tag} ===")
        for idx, name in [(0, "qps"), (2, "p99"), (3, "cpu/req"), (4, "rss_mb")]:
            A = [a[idx] for a, b in pairs]; B = [b[idx] for a, b in pairs]
            rs = [b[idx] / a[idx] for a, b in pairs if a[idx] > 0]
            lo, hi = ci(rs)
            if bad > 0:
                v = "invalid"
            elif name == "qps":
                v = "PASS (non-inf 3%)" if lo > 0.97 else "NOT ESTABLISHED"
            elif name in ("p99", "cpu/req"):
                v = lower_better(lo, hi)
            else:
                v = ""
            print(f"  {name:7} A_med={statistics.median(A):9.1f} B_med={statistics.median(B):9.1f}"
                  f"  B/A_mean={statistics.mean(rs):.3f} 95%CI=[{lo:.3f},{hi:.3f}] {v}")
if __name__ == "__main__":
    for p in sys.argv[1:]:
        run(p)
