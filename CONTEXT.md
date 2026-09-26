# CONTEXT.md — 领域词汇表与模块地图

> 本文件是项目的**唯一领域词汇来源**。AI 与新人接手先读 AGENTS.md → docs/HANDOFF.md 快速接手卡 → 本文件。
> 术语第一次出现时请用这里的名字；发现新概念 → **先补进本文件再写代码**。
> **加功能先看 `docs/cookbook.md` 扩展食谱**；架构契约由 `tests/architecture_tests.rs` 机器看守。

## 一、产品与角色

| 术语 | 含义 |
|------|------|
| **宿主** | 本仓库编译出的 `SorahkDFO.exe`（DfoVibration-V3）：DNF 游戏的连发 + 手柄震动工具，单文件 exe |
| **注入侧 DLL** | 独立仓库的 `DfoVibration_OLD.dll` / `DfoVibration.dll`，被注入游戏进程采集战斗事件（另见 `docs/ACT全职业特调表_v19.md`） |
| **客户端形态表** | `auto_inject.rs` 的 `CLIENTS`：老一代 (ACT1/4/5/40JP/60US → OLD.dll) 与新一代 (90CN → DfoVibration.dll) |
| **路线 S1/S4** | 震动参数的两套隔离世界：S1 = 老版客户端 (ACT 系, `vib_legacy_client=true`)，S4 = 新版。`find_class_for_route` 保证职业预设不跨路线生效 |
| **测试守卫** | `SORAHK_NO_AUTO_INJECT=1` —— 视觉验收/截图脚本**必须带**，否则会真注入游戏客户端 |

## 二、连发域（turbo）

| 术语 | 含义 |
|------|------|
| **连发** | 触发键按住 → 目标键重复敲击；`KeyMapping`（触发键/目标键 SmallVec/间隔/时长/连发开关/备注） |
| **序列宏** | 目标键替换为序列脚本（`sequence.rs` 引擎，`KeySequenceToggle/Pause/Continue` 控制键）；v24.31 起"序列接管输出"（与目标键互斥） |
| **通用宏** | 序列文本里 `宏(名字)` 引用的公共片段（v24.32，连发页管理） |
| **预设切换键** | 一键切换连发预设的热键；`preset_switch_conflict` 家族测试锁定冲突语义 |
| **注入安全保险** | `state/mod.rs` 顶部滑动窗口（≤900 注入单元/秒，超限冷却 1 秒），防连发失控锁死键鼠 |

## 三、震动域（vibration）

| 术语 | 含义 |
|------|------|
| **60 槽参数** | 引擎参数向量：0-18 基础具名（attack/damage/.../rhythm），19-59 高级（= config.advanced[槽-19] 位置对应）。**下标常量 P_* 与 config↔槽位映射的唯一写法在 `src/vibration/params.rs`** |
| **L/R 马达** | 低频/高频马达；`item_lr[26]`（各基础项权重）与 `rank_lr[30]`（评分族权重）成对调控 |
| **事件通道** | DLL 上报的战斗事件：命中 0x01 / 伤害飘字 / 评分 / 怪物死亡 / 移动 …（`VEV_*` 事件号，`push_event` 消费） |
| **ACT 梯度** | ACT 老客户端的逐职业特调体系（ActTier/ActSignature/act_ladder，config.rs 979-1364）；`act_build_params_from` 重建 60 槽 |
| **特供预设** | 「ACT1 特供」= 用户实机调优定稿的通用预设；`act1_base_params()` 是 v19.5 定稿快照（改它会连带 39 个职业变体） |
| **职业预设** | `JobVibration.toml`：全职业 preset（base_job/class_name/applied/params[60]），应用时落 `act1_preset_extras` 附加标量 |
| **共享内存** | `Local\DfoVibrationShm`：DLL 写事件 ring → 宿主震动线程读（布局契约 = `common/vib_protocol.h`，含 size 断言测试） |
| **绝对震动频率** | 召唤职业专属：开启后一切算法失效、按频率直震（abs_freq_*） |
| **群怪不打手** | S1 多怪命中治理算法（v14.1-v16.5），高级调校区小节 |
| **评分动态衰减** | 类鬼泣：越打越猛、停手跌分（v29） |

## 四、持久化文件（serde 字段名 = 冻结契约）

| 文件 | 内容 |
|------|------|
| `Config.toml` | 主配置（连发映射/语言/白名单/设备基线/device_api…） |
| `Vibration.toml` / `Vibration-ACT.toml` | S4 / S1 各自的 `VibrationConfig` + 预设列表（路线隔离） |
| `JobVibration.toml` | 职业预设（applied 时覆盖默认；切路线只重算内存不改写） |

写入一律 `write_atomic`（tmp+rename）；损坏震动文件 → `.bad` 隔离后按默认继续（v24.15）。

## 五、GUI 域（egui 0.33）

| 术语 | 含义 |
|------|------|
| **设计系统** | `src/gui/theme.rs` 是颜色/字号/间距**唯一来源**；`design.md` + `docs/DESIGN.md` 是规范文本。业务代码禁止散落色值字面量 |
| **双形态** | 完整界面（侧边栏 Rail + Topbar）与极简模式（`classic_mode.rs` / `minimal.rs`，都是 `impl SorahkGui` 皮肤变体） |
| **GpFlow** | 手柄页交互状态机（`gui/types.rs`，唯一真相）；输入边沿在 `flow.rs` 处理，渲染不碰识别开关 |
| **两步快速设置** | 手柄页：捕获手柄键做触发 → 捕获键盘/鼠标做目标；`PadCaptureReconciler` 按压窗口合并两通道重复上报 |
| **预设快照** | `temp_config`（设置弹窗）/ `vib_job_snapshot`（职业页滑块自动落盘）——弹窗语义 = 打开拷贝、保存生效、取消丢弃 |

## 六、模块地图（2026-09-27 架构重构 R1 后）

```
src/
├── main.rs              装配: 线程启动顺序 / 托盘 / 信号 / auto_inject
├── config.rs            AppConfig·KeyMapping·VibrationConfig·ACT 梯度·持久化 (serde 字段=冻结契约)
├── state/               AppState 运行态 (一把 Arc 被全部线程共享; OnceLock 全局单例)
│   ├── mod.rs           结构体(122字段)+new+reload+访问器+注入保险+全局单例
│   ├── types.rs         共享类型字典 (EventDispatcher/InputEvent/LiveHidState…) pub use 再导出
│   ├── events.rs        键盘/鼠标钩子事件入口 (handle_key_event/handle_mouse_event)
│   ├── turbo.rs         连发状态机 (组合键激活/修饰键释放/按键录制)
│   ├── inject.rs        SendInput 系统注入 (simulate_action/press/release)
│   ├── mappings.rs      create_input_mappings
│   ├── key_names.rs     名称解析 + SCANCODE_MAP
│   ├── whitelist.rs     前台进程识别 + 进程白名单 (v24.29 路径条目语义)
│   ├── vib_mirror.rs    震动参数应用 (apply_vibration_config, 映射委托 params.rs)
│   └── tests.rs         内嵌测试 (原 state.rs mod tests)
├── vibration/           震动引擎
│   ├── mod.rs           引擎线程循环 run() / push_event / tick / SHM 布局 / send_vibration
│   └── params.rs        ★P_* 槽位常量 + config↔槽位双向映射宏表 (单一事实源)
├── keyboard.rs          WH_KEYBOARD_LL 钩子 + WorkerPool (实现 EventDispatcher → SendInput)
├── mouse.rs             WH_MOUSE_LL 钩子
├── input_manager.rs     设备线程编排 (xinput_thread + rawinput_thread)
├── input_ownership.rs   XInput/RawInput 设备归属仲裁
├── rawinput.rs          WM_INPUT 循环 / HID 报文池 / 设备基线与激活
├── hid_layout.rs        HID 报告描述符解析
├── xinput.rs            XInput 动态加载 / 5ms 轮询 / 组合键 / 震动出口
├── sequence.rs          序列宏引擎 (run_sequence / 录制 poller)
├── auto_inject.rs       客户端形态表 + 注入 (等 DNF.exe 出现)
├── tray.rs / signal.rs / safety.rs   托盘 / Ctrl 信号 / 急退热键
├── util.rs              监督线程 / 写锁工具 / likely-unlikely
├── i18n.rs              四语言文案表 (EN/简中/繁中/日; GUI 现以中文为主, 部分回退 — ADR-0005 前别投资)
└── gui/                 egui 前端 (~2.3 万行, 全部按页面目录化)
    ├── mod.rs           SorahkGui 结构体 + run() + keepalive/svg-prewarm 线程
    ├── main_window.rs   eframe::App / render_shell 双形态分发 / 自绘标题栏
    ├── theme.rs / widgets.rs / types.rs / fonts.rs / utils.rs   设计系统与共享小件
    ├── guide.rs         首次运行向导
    ├── classic_mode.rs / minimal.rs             双形态皮肤
    ├── settings_dialog.rs        设置弹窗 (主编排 + 6 个分区方法)
    ├── vibration_page/  通用震动+全职业预设页 (page/sections/hints/export/presets)
    ├── gamepad_mapping/ 手柄可视化映射页 (model/capture/helpers/svg/page/quick_connect/flow/panel)
    ├── turbo_page/      连发映射页 (page/presets/edit/macros)
    └── about/error/hid_activation/mouse_*/whitelist/device_*/keyboard_quick   其余弹窗与页
```

**依赖方向（无环，编译器在看管）**：`gui → state → config`；`keyboard/mouse → state`；`rawinput/xinput → state`；`vibration → state + xinput`；`main → 全部装配`。
**守卫升级 (R1.1)**：方向不再只靠自觉 —— `tests/architecture_tests.rs` 机器看守（GUI 外禁
crate::gui / gui 依赖允许清单 / config 叶子化 / serde 契约快照）。另: `tests/` 7+1 个集成
测试文件是行为守卫（config 往返/路线隔离/职业变体校验和…），全绿基线见 HANDOFF。

## 七、线程模型（全部共享 `Arc<AppState>`，以原子量为主）

主线程(GUI) · keyboard_hook · mouse_hook · worker×N(连发注入) · seq-runner(按需) · xinput_thread · rawinput_thread · vibration_thread · auto_inject · tray · key-record-poller · svg-prewarm · ui-keepalive。
GUI 与引擎的唯一耦合面 = 60 槽原子参数 + 具名原子量；这正是 params.rs 单一事实源存在的理由。
