# DfoVibration-V3 (主程序)

DNF 手柄映射 + 震动反馈工具（基于 [Sorahk](https://github.com/llnut/Sorahk) 二次开发）:

- **连发**: 键盘/鼠标/手柄触发 → 目标键, 多线程涡轮连发, 组合键/双击/多重动作映射
- **震动**: 外部 DLL 采集游戏战斗事件 (共享内存环形队列) → Xbox 手柄马达,
  15 种职业算法 / 60 项参数 / 13+15 项左右马达权重, 全部可调
- 技术栈: Rust + egui 0.33 + eframe(glow) + windows-rs, 单文件 exe, 413 个测试

## 构建

中文路径下 MinGW 链接会失败, 必须用 ASCII 构建目录 (脚本自动同步+构建):

```bash
bash sync_and_build.sh                        # 同步到 E:\Sorahk-build 并 cargo build --release
cd /e/Sorahk-build && cargo test --release    # 全量测试
```

运行时交付物 (exe / Config.toml / Vibration.toml / DfoVibration.dll) 在上层运行时目录, 不入库。

## 文档索引 (docs/)

| 文档 | 内容 |
|---|---|
| [HANDOFF.md](docs/HANDOFF.md) | **交接文档: 构建铁律 / 协作注意 / egui 踩坑清单, 接手必读** |
| [DESIGN.md](docs/DESIGN.md) · [../design.md](../design.md) | UI 设计规范 (与 theme.rs 同步维护) |
| [震动系统开发规范_v20.md](docs/震动系统开发规范_v20.md) | 参数体系 / 存储 / UI / 预设 / 事件协议 唯一参考 |
| [全职业预设_v21.md](docs/全职业预设_v21.md) | 全职业两级预设体系 |
| [稳定性修复记录_v1.6.md](docs/稳定性修复记录_v1.6.md) | 稳定性体系 (panic 兜底 / supervised 线程) |
| [震动页UI设计规范.md](docs/震动页UI设计规范.md) | 震动页 UI 规范 |
| [使用说明.txt](docs/使用说明.txt) · [更新说明.txt](docs/更新说明.txt) | 终端用户文档 |

`AGENTS.md` 是 agent 工程技能配置入口; 逆向依据 (地址/函数定位) 在逆向研究仓库。

## 分享预设

- `Config.toml` (连发映射/预设) 与 `Vibration.toml` (震动参数/自建预设) 直接发给对方即可,
  字段由 serde 全量落盘, 无隐藏丢失
- 全职业微调快照在运行时目录的 `JobVibration.toml`;
  跨机分享用程序导出的 `job_export_*.toml` / `vibration_export_*.toml`

## 相关仓库

- **DfoVibration-DLL**: 游戏内事件采集 DLL 源码 (共享内存生产者)
- **逆向研究仓库**: DNF 战斗系统逆向分析 / IDA 报告 / 实施记录 (本仓库震动功能的依据)
