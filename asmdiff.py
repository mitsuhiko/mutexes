#!/usr/bin/env python3
"""Compare stock and shared probe assembly.

usage: asmdiff.py STOCK.s SHARED.s

Checks that the `PROBE_LAYOUT` static (sizes, alignments and auto traits) has the same
bytes on both sides, then prints each function whose instructions differ, with
instruction counts and whether its atomics and calls are the same multiset.
Exits non-zero if the layout differs or a function's atomics or calls do.
"""
import collections
import re
import sys

# Local labels: `L…`/`l…` on Mach-O, `.L…` on ELF, `$…` on COFF.
LABEL = re.compile(r"^(_?(?![Ll]|\.L|\$)[A-Za-z_][\w.$@?]*):")
ANY_LABEL = re.compile(r"^[^\s#;@][^\s]*:")
END = re.compile(r"^\s*(\.cfi_endproc|\.seh_endproc|end_function)\b|^\.Lfunc_end")
DATA = re.compile(r"^\s*\.(asciz|ascii|byte|short|hword|long|word|quad|int8|int16|int32|int64|zero|space)\b")
ATOMIC = re.compile(r"atomic|^amo|^lr\.|^sc\.|^(ldapr|ldar|stlr|cas|swp|ldadd|ldclr|ldset|ldxr|ldaxr|stxr|stlxr|lock|xchg)")
CALL = re.compile(r"^(bl|blr|call|callq|jal|tail|return_call)$")


def parse(path):
    functions, data, cur, in_data = {}, {}, None, None
    for line in open(path):
        m = LABEL.match(line)
        if m:
            name = m.group(1)
            cur, in_data = functions.setdefault(name, []), data.setdefault(name, [])
            continue
        if ANY_LABEL.match(line):
            in_data = None  # a local label starts some other data
        if END.match(line):
            cur = in_data = None
            continue
        t = line.strip()
        if in_data is not None and DATA.match(line):
            in_data.append(t)
        if cur is not None and t and not t.startswith((".", ";", "#", "@")) and not t.endswith(":"):
            cur.append(t)
    return {k: renumber(v) for k, v in functions.items() if v}, data


LOCAL = re.compile(r"\.?L(?:BB|tmp|func_end|loh|lloh|exception|BB_END)?[0-9_]+\b|\.?Lloh[0-9]+")


def renumber(body):
    """Local labels are numbered per file; number them per function instead."""
    seen = {}
    return [LOCAL.sub(lambda m: seen.setdefault(m.group(0), f"L{len(seen)}"), ins) for ins in body]


def effects(body):
    out = collections.Counter()
    for ins in body:
        op, *args = ins.split(None, 1)
        if ATOMIC.search(op):
            out[op] += 1
        elif CALL.match(op) or (op in ("b", "j", "jmp") and args and "_R" in args[0]):
            out[ins] += 1
    return out


(old, old_data), (new, new_data) = parse(sys.argv[1]), parse(sys.argv[2])
layout = [k for k in old_data if k.lstrip("_") == "PROBE_LAYOUT"]
if not layout or not old_data[layout[0]]:
    sys.exit("no PROBE_LAYOUT static found")
if old_data[layout[0]] != new_data.get(layout[0]):
    sys.exit("PROBE_LAYOUT differs: sizes, alignments or auto traits changed")
print("Sizes, alignments and auto traits are identical.")

failed, same = False, 0
names = sorted(old.keys() | new.keys())
for name in names:
    a, b = old.get(name), new.get(name)
    if a == b:
        same += 1
    elif a is None or b is None:
        print(f"  {name}: only in {'shared' if a is None else 'stock'}")
        failed = True
    elif effects(a) == effects(b):
        print(f"  {name}: {len(a)} -> {len(b)} instructions, same atomics and calls")
    else:
        print(f"  {name}: {len(a)} -> {len(b)} instructions, DIFFERENT atomics or calls")
        failed = True
print(f"{same} of {len(names)} functions identical")
sys.exit(failed)
