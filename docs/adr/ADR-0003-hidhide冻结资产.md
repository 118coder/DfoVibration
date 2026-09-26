# ADR-0003: hidhide（手柄隐藏）冻结资产移出编译目录

- 状态: 已采纳 (2026-09-27；功能下线本身 = v24.19b 用户决策 2026-09-14)
- 背景: `hidhide.rs` 1005 行整体注释下线后仍留在 `src/`，AI/检索会把它误当活代码阅读。
- 决策: 文件移至 `docs/frozen/hidhide.rs`；`src/lib.rs` / `src/main.rs` / `src/config.rs`
  的注释指向新路径；文件头 banner 增加"步骤 0：先移回 src/hidhide.rs"。
- 恢复流程（完整 5+1 步）见 `docs/frozen/hidhide.rs` 文件头；恢复后必须重跑
  `work/_e2e_hidhide.py`（真驱动 1.4.181 + 真手柄 9/9 基线）。
- 注: config.rs 中注释掉的 `hidhide_*` 字段块保留原位（serde 忽略旧字段，无迁移）。
