# -*- coding: utf-8 -*-
"""E 盘主线 → D 盘 clone 同步 (HANDOFF 第42条: 按 E 盘 git ls-files 清单逐文件覆盖 + 删多余)"""
import subprocess, os, shutil

E = r"E:\网页小工具\DfoVibration V3版本\SorahkDFO源码"
D = r"D:\Program Files\Gitgub\DfoVibration"

def ls_files(repo):
    """git ls-files -z: 原始路径名, 不转义中文 (core.quotepath 默认会把非 ASCII 转成 \351 灾难)"""
    out = subprocess.check_output(["git", "-C", repo, "ls-files", "-z"])
    return [p for p in out.decode("utf-8").split("\0") if p]

e_files = ls_files(E)
e_set = set(e_files)

copied = 0
for rel in e_files:
    src = os.path.join(E, rel)
    dst = os.path.join(D, rel)
    if not os.path.isfile(src):
        print("!! E 清单内缺失:", rel)
        continue
    os.makedirs(os.path.dirname(dst) or D, exist_ok=True)
    shutil.copy2(src, dst)
    copied += 1

d_files = ls_files(D)
removed = 0
for rel in d_files:
    if rel in e_set:
        continue
    p = os.path.join(D, rel)
    if os.path.isfile(p):
        os.remove(p)
        print("rm 多余:", rel)
        removed += 1

print(f"copied={copied} removed={removed}")
