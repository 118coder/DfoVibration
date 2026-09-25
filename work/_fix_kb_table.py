# -*- coding: utf-8 -*-
"""v2: 修复 keyboard_quick.rs KEYS 表坐标 (以 SVG rect 为唯一真值)。
写入后重新读盘验证, 任何一步失败即退出非零。"""
import re, io, sys, os

ROOT = r"E:\网页小工具\DfoVibration V3版本\SorahkDFO源码"
RS = ROOT + r"\src\gui\keyboard_quick.rs"

def parse_svg(path):
    txt = io.open(path, encoding="utf-8").read()
    out = {}
    for m in re.finditer(r'<g id="([^"]+)">(.*?)</g>', txt, re.S):
        gid, body = m.group(1), m.group(2)
        r = re.search(r'<rect x="([-\d.]+)" y="([-\d.]+)" width="([-\d.]+)" height="([-\d.]+)"', body)
        if r:
            out[gid] = tuple(float(v) for v in r.groups())
    return out

dark = parse_svg(ROOT + r"\resources\keyboard.svg")
print("SVG 键数:", len(dark))

src = io.open(RS, encoding="utf-8").read()
pat = re.compile(
    r'KeyboardKey \{ name: "([^"]+)", label: "((?:[^"\\]|\\.)*)", '
    r'x: [-\d.e+]+, y: [-\d.e+]+, w: [-\d.e+]+, h: [-\d.e+]+ \},'
)
matches = pat.findall(src)
print("表条目数:", len(matches))

unmatched = []
def repl(m):
    name, label = m.group(1), m.group(2)
    geo = dark.get(name) or dark.get(name.upper())
    if geo is None:
        unmatched.append(name)
        return m.group(0)
    x, y, w, h = geo
    # 注意: Python 的 "{:.1}" 对浮点是 g 通用格式 (1 位有效数字 → 3e+01), 必须用 ".1f"
    return 'KeyboardKey {{ name: "{}", label: "{}", x: {:.1f}, y: {:.1f}, w: {:.1f}, h: {:.1f} }},'.format(
        name, label, x, y, w, h)

new = pat.sub(repl, src)
if unmatched:
    print("!! 未匹配:", unmatched); sys.exit(1)

left = new.count("e+0")
print("替换后残留 e+0:", left)
if left:
    for l in new.splitlines():
        if "e+0" in l:
            print("  残留行:", l)
    sys.exit(1)

with io.open(RS, "w", encoding="utf-8", newline="\n") as f:
    f.write(new)
    f.flush()
    os.fsync(f.fileno())

# 重新读盘验证
disk = io.open(RS, encoding="utf-8").read()
assert disk.count("e+0") == 0, "写盘验证失败: 磁盘仍含 e+0"
assert disk.count("KeyboardKey {") == len(matches) + 1, "条目数异常"
# 抽查
for probe in ("ESC", "S", "D", "F", "G", "H"):
    m2 = re.search(r'name: "%s", label: "(?:[^"\\]|\\.)*", x: ([\d.]+), y: ([\d.]+), w: ([\d.]+), h: ([\d.]+)' % probe, disk)
    print("  %-6s x=%-6s y=%-6s w=%-6s h=%-6s  svg=%s" % ((probe,) + m2.groups() + (dark[probe],)))
print("OK: 102 键坐标已按 SVG 回填")
