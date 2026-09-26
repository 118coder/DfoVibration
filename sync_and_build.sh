#!/bin/bash
# 同步全部源文件到构建目录并构建 (中文路径无法直接编译)
# pipefail 必须开: `| tail` 会掩码 cargo 退出码, 曾两次把旧包当新包交付 (见 HANDOFF 交付纪律)
set -e
set -o pipefail
SRC="/e/网页小工具/DfoVibration V3版本/SorahkDFO源码"
DST="/e/Sorahk-build"

# ★架构重构 A (2026-09-27): 日常迭代默认走 iter 快速档 (opt-1/无 fat-LTO/16 CGU/增量),
#   交付构建显式 `bash sync_and_build.sh --release` (fat-LTO+单CGU 照旧)。改码→运行循环快数倍。
MODE="--profile iter"
if [ "$1" = "--release" ]; then MODE="--release"; fi

# ★2026-09-27: 会话沙箱可能把 CARGO_HOME 重定向到含中文的缓存路径 —— MinGW ld 打不开
#   非 ASCII 路径下的导入库 (报 cannot find -lwindows.0.53.0, 即本项目"中文路径 MinGW
#   链接失败"铁律的变体)。固定用用户目录 ASCII cargo home (与历史 release 构建同一份缓存);
#   不存在时退回环境默认并给出显式提示。
if [ -d "/c/Users/12290/.cargo/registry" ]; then
    export CARGO_HOME="/c/Users/12290/.cargo"
fi

# ★先删后拷: 目录化拆分后, 源码树里删除/改名的文件不得在构建目录残留
#   (残留会导致 rustc 对同一模块同时找到 state.rs 与 state/mod.rs 之类错乱)
rm -rf "$DST/src" "$DST/tests"
# 递归同步整个 src/ (含 gui/ 与 job_presets/ 等子目录; 旧版只拷顶层 *.rs 会漏掉子目录改动)
cp -r "$SRC/src" "$DST/src"
# resources 全量同步 (gamepad.svg + sorahk.ico/sorahk.rc 图标资源, 漏拷会导致图标改动不生效)
cp -r "$SRC/resources/." "$DST/resources/"
cp "$SRC/Cargo.toml" "$DST/Cargo.toml"
cp "$SRC/build.rs" "$DST/build.rs"
cp -r "$SRC/tests" "$DST/tests" 2>/dev/null || true
cd "$DST"
# ★v24.20a (用户决策 2026-09-15): 宿主**不**嵌 highestAvailable manifest ——
# 默认普通权限运行; 90CN 客户端由使用说明指引右键管理员运行。
# 如未来要恢复静默提权: SORAHK_MANIFEST=1 cargo build (resources/sorahk_manifest.rc)。
cargo build $MODE 2>&1 | tail -1
