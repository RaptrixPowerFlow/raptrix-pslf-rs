#!/usr/bin/env python3
from collections import defaultdict
import sys

epc = sys.argv[1]
in_svd = False
pairs_by_bus = defaultdict(list)
lines = open(epc).read().splitlines()
i = 0
while i < len(lines):
    line = lines[i]
    if line.lower().startswith("svd data"):
        in_svd = True
        i += 1
        continue
    if not in_svd:
        i += 1
        continue
    if "data  [" in line.lower() and not line.strip().lower().startswith("svd"):
        break
    s = line.strip()
    if s and s[0].isdigit() and ":" in s:
        bus = s.split()[0]
        if i + 1 < len(lines):
            cont = lines[i + 1].strip()
            parts = cont.split()
            try:
                n = int(float(parts[0]))
                b = float(parts[1])
                pairs_by_bus[bus].append((n, b))
            except (IndexError, ValueError):
                pass
    i += 1

total_banks = sum(sum(n for n, _ in v) for v in pairs_by_bus.values())
print(f"unique buses: {len(pairs_by_bus)}")
print(f"total bank step rows (sum N): {total_banks}")
print(f"total svd records parsed as headers: {sum(len(v) for v in pairs_by_bus.values())}")
