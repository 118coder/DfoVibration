# ADR-0001: 拆分采用单 crate 目录模块，不做 cargo workspace

- 状态: 已采纳 (2026-09-27)
- 背景: 架构评审提出两个候选 —— B/C/G「文件级拆分」与 E「cargo workspace 多 crate」。
- 决策: 先做文件级拆分（state/gui 四大页面/vibration 目录化），**不做** workspace。
- 理由:
  1. 痛点主因是构建流程（日常也走 fat-LTO release + 测试跑 4 遍），`--profile iter` 已把
     改→测循环降到秒级，workspace 的编译收益在当前规模下边际；
  2. workspace 要动 `Arc<AppState>` 跨 crate、EventDispatcher trait 归属、cfg(test) 迁移，
     与"功能零变化"目标冲突，风险/收益不成比例；
  3. 依赖图无环且方向单一（CONTEXT.md §六），未来若拆，接缝已就位。
- 重新评估触发条件: iter 档后日常构建实测仍 >3 分钟；或出现需要编译器强制禁止的跨界引用
  （如 gui 触碰 rawinput 内部）。
