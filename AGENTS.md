# AGENTS.md — DfoVibration-V3（SorahkDFO 源码）

基于 [Sorahk](https://github.com/llnut/Sorahk) 的 DNF 游戏工具（连发 + Xbox 手柄震动），技术栈 Rust + egui 0.33.3 + eframe(glow) + windows-rs，单文件 exe。GUI 设计系统唯一来源在 `src/gui/theme.rs`，设计规范见 `design.md`。

## 接手必读（按顺序）

1. **`docs/开发维护规范.md`** — 开发/维护的强制性规范：构建铁律（中文路径→E:\Sorahk-build、iter 档、ASCII CARGO_HOME/TMP）、修改流程纪律（一次一事四步闭环）、代码规范（模块组织/可见性/serde 冻结契约）、测试守卫清单、上传纪律。
2. **`docs/HANDOFF.md`** — 交接文档：当前版本状态、快速接手卡、历史病历与实战要点。
3. **`CONTEXT.md`** — 领域词汇表 + 模块地图（改架构/加概念必同步更新它）。
4. **`docs/adr/`** — 已定决策（单 crate 不 workspace、震动参数单一事实源、hidhide 冻结、构建管线纪律）；重新提议这些方向前先读。
5. `design.md` + `docs/DESIGN.md` — UI 设计规范（新页面必读）。

## 构建注意（铁律，详见规范 §1）

- **只在 `E:\Sorahk-build` 构建**（中文路径 MinGW 链接失败）：`bash sync_and_build.sh` 默认 iter 快速档；交付才 `--release`。
- 会话内手动跑 cargo 必须带 ASCII 环境：`CARGO_HOME=/c/Users/12290/.cargo TMP=/e/Sorahk-build/.tmp TEMP=/e/Sorahk-build/.tmp`。
- 测试：`run_tests.bat`（一遍制）；环境敏感测试（真实按键注入类）锁屏/无人值守会假失败。

## Agent skills

### Issue tracker

Issues 以本地 markdown 文件形式存放在 `.scratch/<feature>/` 下（本仓库无远程 git 仓库）。见 `docs/agents/issue-tracker.md`。

### Triage labels

五个标准 triage 角色，标签字符串与角色名相同，以 `Status:` 行写在 issue 文件顶部。见 `docs/agents/triage-labels.md`。

### Domain docs

Single-context 布局：仓库根 `CONTEXT.md` + `docs/adr/`（R1 已落地）。见 `docs/agents/domain.md`。
