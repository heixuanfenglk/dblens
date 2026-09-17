# -*- coding: utf-8 -*-
"""Scan all Chinese string literals in src/**/*.rs"""
from pathlib import Path
import re
import json

ROOT = Path(r"d:\repos\allink\src")
cjk = re.compile(r"[\u4e00-\u9fff]")
# rust string literals (simple)
str_re = re.compile(r'"(?:\\.|[^"\\])*"')

results = []
for p in sorted(ROOT.rglob("*.rs")):
    if p.name == "ui_text.rs":
        continue
    text = p.read_text(encoding="utf-8")
    for i, line in enumerate(text.splitlines(), 1):
        # skip comments-only lines for listing but still migrate comments? user said 代码里出现的中文 - include strings primarily
        for m in str_re.finditer(line):
            lit = m.group(0)
            if cjk.search(lit):
                results.append({
                    "file": str(p.relative_to(ROOT)).replace("\\", "/"),
                    "line": i,
                    "lit": lit,
                    "in_comment": line.strip().startswith("//"),
                })

Path(r"d:\repos\allink\_cjk_scan.json").write_text(
    json.dumps(results, ensure_ascii=False, indent=2), encoding="utf-8"
)
# summary by file
from collections import Counter
c = Counter(r["file"] for r in results)
print("total", len(results))
for f, n in c.most_common():
    print(f"{n:4d} {f}")
