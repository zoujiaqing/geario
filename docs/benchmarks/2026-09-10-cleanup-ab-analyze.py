#!/usr/bin/env python3
"""Paired A/B analysis for the phase-two cleanup no-regression check.

A = geario-before (588c0d3), B = geario-after (2378a15). Per round the two
arms are measured seconds apart; the paired ratio B/A controls host drift.

Acceptance is NON-INFERIORITY at a pre-registered 3% margin: the cleanup is
accepted for a load only if the 95% bootstrap CI lower bound of mean(B/A) is
above 0.97. A CI that merely crosses 1.0 is NOT a pass -- it only means no
change was detected, which at these CI widths still admits a real regression.

Usage: analyze.py <raw.txt> [<raw.txt> ...]   (reads ab2.sh round lines)
"""
import re, sys, random, statistics

MARGIN = 0.97  # pre-registered 3% non-inferiority threshold

def parse_blocks(text):
    # Split on "##### <size> #####" headers; fall back to one unnamed block.
    parts = re.split(r'#{3,}\s*(\S+?)\s*#{3,}', text)
    if len(parts) == 1:
        return [("(all)", text)]
    return [(parts[i], parts[i + 1]) for i in range(1, len(parts), 2)]

def rounds_of(data):
    rs = {}
    for line in data.splitlines():
        m = re.match(r'(\d+)\s+(A|B)\s+(\d+)\s+([\d.]+)\s+(\d+)', line.strip())
        if m:
            rs.setdefault(int(m.group(1)), {})[m.group(2)] = (int(m.group(3)), float(m.group(4)), int(m.group(5)))
    return rs

def analyze(label, data):
    rs = rounds_of(data)
    pairs = [(v['A'], v['B']) for v in rs.values() if 'A' in v and 'B' in v]
    if not pairs:
        return
    A = [a[0] for a, b in pairs]; B = [b[0] for a, b in pairs]
    bad = sum(a[2] + b[2] for a, b in pairs)
    ratios = [b[0] / a[0] for a, b in pairs]
    random.seed(1)
    boots = sorted(sum(random.choice(ratios) for _ in ratios) / len(ratios) for _ in range(10000))
    lo, hi = boots[250], boots[9750]
    verdict = "PASS (non-inferior at 3%)" if lo > MARGIN else "NOT ESTABLISHED (CI admits >3% regression)"
    print(f"=== {label}: {len(pairs)} paired rounds, errors+mismatches={bad} ===")
    print(f"  before(A) qps mean={statistics.mean(A):.0f} median={statistics.median(A):.0f}")
    print(f"  after (B) qps mean={statistics.mean(B):.0f} median={statistics.median(B):.0f}")
    print(f"  B/A mean={statistics.mean(ratios):.4f} 95%CI=[{lo:.4f},{hi:.4f}]  margin={MARGIN}")
    print(f"  => {verdict}")

for path in sys.argv[1:]:
    for label, data in parse_blocks(open(path).read()):
        analyze(f"{path.split('/')[-1]} {label}", data)
