#!/bin/bash
# 同步全部源文件到构建目录并构建 (中文路径无法直接编译)
# pipefail 必须开: `| tail` 会掩码 cargo 退出码, 曾两次把旧包当新包交付 (见 HANDOFF 交付纪律)
set -e
set -o pipefail
SRC="/e/网页小工具/DfoVibration V3版本/SorahkDFO源码"
DST="/e/Sorahk-build"
# 递归同步整个 src/ (含 gui/ 与 job_presets/ 等子目录; 旧版只拷顶层 *.rs 会漏掉子目录改动)
cp -r "$SRC/src/." "$DST/src/"
cp "$SRC/resources/gamepad.svg" "$DST/resources/"
cp "$SRC/Cargo.toml" "$DST/Cargo.toml"
cp -r "$SRC/tests/." "$DST/tests/" 2>/dev/null || true
cd "$DST"
cargo build --release 2>&1 | tail -1
