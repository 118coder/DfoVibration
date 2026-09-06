# AGENTS.md — DfoVibration-V3（SorahkDFO 源码）

基于 [Sorahk](https://github.com/llnut/Sorahk) 的 DNF 游戏工具（连发 + Xbox 手柄震动），技术栈 Rust + egui 0.33.3 + eframe(glow) + windows-rs，单文件 exe。GUI 设计系统唯一来源在 `src/gui/theme.rs`，设计规范见 `design.md`。

**接手前必读**：`docs/HANDOFF.md`（交接文档：构建铁律、协作注意事项、egui 踩坑清单）· `design.md`（UI 设计规范，新页面必读）· `REFACTOR_PLAN.md`（历史日志）。

构建注意：中文路径下 MinGW 链接会失败，必须走 ASCII 构建目录 `E:\Sorahk-build\`（`bash sync_and_build.sh` 同步+构建）。

## Agent skills

### Issue tracker

Issues 以本地 markdown 文件形式存放在 `.scratch/<feature>/` 下（本仓库无远程 git 仓库）。见 `docs/agents/issue-tracker.md`。

### Triage labels

五个标准 triage 角色，标签字符串与角色名相同，以 `Status:` 行写在 issue 文件顶部。见 `docs/agents/triage-labels.md`。

### Domain docs

Single-context 布局：仓库根 `CONTEXT.md` + `docs/adr/`。见 `docs/agents/domain.md`。
