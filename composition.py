#!/usr/bin/env python3
"""Turn `-Zprint-type-sizes` output (stdin) into the composition table."""
import re
import sys

sizes = {t: int(n) for t, n in re.findall(r"^print-type-size type: `(.*)`: (\d+) bytes", sys.stdin.read(), re.M)}
payloads = ["()", "u8", "u32", "u64", "u128", "std::boxed::Box<u8>", "std::vec::Vec<u8>",
            "std::string::String", "std::collections::HashMap<u8, u8>", "layout::CacheLine"]
print(f"{'T':16} {'Mutex<T>':>9} {'nonpoison::Mutex<Poison<T>>':>34} {'RefCell<T>':>11} {'RefCell<Poison<T>>':>19}")
for t in payloads:
    p = f"std::sync::poison::Poison<{t}>"
    mutex, composed = sizes[f"std::sync::Mutex<{t}>"], sizes[f"std::sync::nonpoison::Mutex<{p}>"]
    cell, cell_composed = sizes[f"std::cell::RefCell<{t}>"], sizes[f"std::cell::RefCell<{p}>"]
    extra = f"(+{composed - mutex})" if composed != mutex else ""
    name = re.sub(r"\b(std|layout|collections|boxed|vec|string)::", "", t)
    print(f"{name:16} {mutex:>9} {composed:>27} {extra:>6} {cell:>11} {cell_composed:>19}")
