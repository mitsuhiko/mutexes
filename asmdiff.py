#!/usr/bin/env python3
"""Compare two assembly files function by function.

Prints each function whose instructions differ, with instruction counts and
whether its atomic instructions and calls are the same multiset.
"""
import collections
import re
import sys

LABEL = re.compile(r"^(_?(?!LBB|Lloh|Ltmp|LCPI|l_|\.L)[A-Za-z_][\w.$]*):$")
ATOMIC = re.compile(r"^(ldapr|ldar|stlr|cas|swp|ldadd|ldclr|ldset|ldxr|stxr|lock|xchg)")


def functions(path):
    out, cur = {}, None
    for line in open(path):
        m = LABEL.match(line)
        if m:
            cur = out.setdefault(m.group(1), [])
        elif ".cfi_endproc" in line:
            cur = None
        elif cur is not None:
            t = line.strip()
            if t and not t.startswith((".", ";", "#")) and not t.endswith(":"):
                cur.append(t)
    return out


def effects(body):
    ops = (i.split() for i in body)
    return collections.Counter(
        " ".join(o) if o[0] in ("bl", "b", "callq", "jmp") and "@" not in o[-1] and not o[-1].startswith(("LBB", ".LBB")) else o[0]
        for o in ops
        if ATOMIC.match(o[0]) or (o[0] in ("bl", "callq") or (o[0] in ("b", "jmp") and "_R" in o[-1]))
    )


old, new = functions(sys.argv[1]), functions(sys.argv[2])
same = 0
for name in sorted(old.keys() | new.keys()):
    a, b = old.get(name), new.get(name)
    if a == b:
        same += 1
        continue
    if a is None or b is None:
        print(f"  {name}: only in {'new' if a is None else 'old'}")
        continue
    verdict = "same atomics and calls" if effects(a) == effects(b) else "DIFFERENT atomics or calls"
    print(f"  {name}: {len(a)} -> {len(b)} instructions, {verdict}")
print(f"{same} of {len(old.keys() | new.keys())} functions identical")
