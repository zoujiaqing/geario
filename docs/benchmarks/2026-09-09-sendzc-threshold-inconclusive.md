# SendZc threshold for single-page sends: inconclusive, default unchanged

- Date: 2026-09-09. Host: Rocky 9, io_uring on, 16 KB pages, 4 workers.
- Question: single-page sends above 1536 bytes use zero-copy `SendZc`. Does
  turning that off help the 2-8 KB range, where io_uring trails polling?

## Two rotated runs disagreed

**Session 1** (rotate SendZc on/off per size per round, 10 rounds): off won
2 KB +8.5% [+6.2,+10.9], 4 KB +9.2% [+5.7,+12.3], 8 KB +8.4% [+5.4,+11.1],
CIs clear of zero.

**Retest** (same binary, A = zc 1536, B = off, alternating within each round,
12 rounds): 4 KB -0.82% [-3.89,+2.35], 8 KB +0.43% [-1.98,+3.22] -- a tie.

Same comparison, opposite conclusion. The on-config numbers matched across
sessions (~193k at 4 KB both times); the off-config swung (211k session 1,
192k retest). So the apparent 9% was the off runs landing in a quieter
moment on this shared KVM host, not the change.

## Decision

Default left at 1536, unchanged from the ntex port. The rule is that a change
ships only on a reproducible gain, and this one did not reproduce. The
`GEARIO_URING_ZC_SIZE` env knob stays for further measurement, ideally on a
quiet or dedicated host and on a real NIC, where zero-copy's trade-off is
different from loopback.

## Lesson for the harness

Alternating the two arms within each round, seconds apart, is more robust to
host drift than measuring one config's rounds and then the other's. The
session-1 layout still grouped enough work per round to let drift in.
