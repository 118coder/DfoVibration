# -*- coding: utf-8 -*-
"""R2.0 发行包打包: pkg_r2 工作目录 → 正式 zip (arc 分隔符统一 /)"""
import os
import zipfile

out = r"E:/网页小工具/DfoVibration V3版本/（适配ACT手柄震动+连发映射+组合键宏）DfoVibration-Sorahk_R2.0正式版.zip"
base = "E:/Sorahk-build/pkg_r2"
zf = zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED)
count = 0
for root, dirs, files in os.walk(base):
    for f in files:
        if f.endswith(".zip"):
            continue
        full = os.path.join(root, f)
        arc = os.path.relpath(full, base).replace("\\", "/")
        zf.write(full, arc)
        count += 1
zf.close()
print("entries:", count, "size KB:", os.path.getsize(out) // 1024)
print(out)
