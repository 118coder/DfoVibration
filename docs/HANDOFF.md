# DfoVibration-V3 (原 SorahkDFO) — 交接文档 (HANDOFF)

> 更新: 2026-09-07 · 目的: 让下一个对话/会话无需翻历史即可无缝接手
> 配套: `design.md`(设计规范, 新页面必读) · `docs/DESIGN.md` · `REFACTOR_PLAN.md`(历史日志)

---

## 一、项目是什么

**DfoVibration-V3** (内部标识仍为 Sorahk/SorahkGui, 未改): 基于 [Sorahk](https://github.com/llnut/Sorahk) 的 DNF 游戏工具:
- **连发**: 按键映射 (键盘/鼠标/手柄触发 → 目标键), 多线程涡轮连发
- **震动**: DLL 采集游戏战斗事件 → Xbox 手柄马达, 参数可调
- 技术栈: **Rust + egui 0.33.3 + eframe(glow) + windows-rs**, 单文件 exe

目录 `E:\网页小工具\DfoVibration V3版本\`:
- `SorahkDFO源码\` — 全部源码 (GUI 约 9k 行, 设计系统在 `src/gui/theme.rs`) — **独立 git 仓库 (DfoVibration 主程序)**
- `DLL源码\` — 采集 DLL 源码 — **独立 git 仓库 (DfoVibration-DLL)**
- `研究文档\` — 逆向研究资料 — **独立 git 仓库 (逆向研究, 含 00总览/会话记录/IDA 报告)**
- `SorahkDFO-新UI版.exe` — **最新交付版** (文件名沿用旧名, 显示名已改 DfoVibration-V3)
- `Config.toml` / `Vibration.toml` — 用户配置 (App 自管理)

> 2026-09-07 文档重组: 原父目录「开发文档」已按归属并入本仓库 docs/ (震动规范/全职业预设/稳定性记录/UI规范/使用说明/更新说明) 与研究仓库 (00总览/会话记录); UI部件 移入本仓库。

构建目录: `E:\Sorahk-build\` (中文路径 MinGW 链接会失败, 必须用此 ASCII 目录)

## 二、当前状态 (2026-09-07 凌晨)

✅ 174 lib tests 全绿 · 最新 exe 已交付 (构建 00:53 后)
✅ UI 风格 v3.1 "Obsidian Console Vivid" (规范见 `design.md`, 与 theme.rs 完全同步)
✅ 用户已实际使用并逐项验收; 唯一未决: 各项微调 (1px 级) 是否合意, 等用户反馈

### 已完成大项 (时间序)
1. UI 全量重构: 侧边栏导航/类型系统/自绘标题栏/Phosphor 图标/极简模式
2. v3.0 风格迭代: Linear 实证令牌、深浅双主题、真粗体字体族、卡片柔影、顶部微光、悬停动效 (12 子代理调研)
3. v3.1: 色板提饱和、设置单齿轮、改名 DfoVibration-V3、极简震动预设分段开关、窗口位置记忆
4. v3.2: 100% 小窗强制、两级职业选择、连发映射预设管理卡(删除双确认)、首次运行指引、预设逻辑重构
5. v3.3: 全职业两级 combo 垂直对齐(等高槽位)、剑魂微调(显式矩形 class_nudge)、移除「不应用」、分段切换直接启用、**双重 DPI 换算修复**
6. v3.4 (2026-09-07): 遗留缺陷批次 — 共享内存版本校验(VIB_SHM_VERSION=2)/断连整引擎重置/震动输出自动发现槽位(修硬编码 0 号槽)/u32 毫秒回绕安全比较 before()(49.7 天卡死)/EVENT_BACKLOG 发送失败回收/托盘通知通道 OnceLock→Mutex(重启可换 sender)/config serde 全字段落盘迁移 + roundtrip 测试/卡键 P0 批次(闸门放行抬起/worker 暂停补发 release/XInput 断连派发 Released/hook FFI catch_unwind/重启容错)/职业导入长度校验/振动 params 启动恢复
7. v3.2 视觉 (2026-09-07): 按钮语义层级强制 (规范见 design.md v3.2 增补) — 设置弹窗 保存绿/取消红 → primary/secondary, 三套强调色字面量收敛到主题令牌, 测试评分震动按钮入体系

## 三、构建与验证流程 (铁律)

```bash
# 1. 修改源码: E:\网页小工具\DfoVibration V3版本\SorahkDFO源码\src\gui\
# 2. 同步+构建 (中文路径 MinGW 链接失败, 必须走 ASCII 构建目录):
bash "E:\网页小工具\DfoVibration V3版本\SorahkDFO源码\sync_and_build.sh"
# 3. 测试: cd /e/Sorahk-build && cargo test --release --lib   (174 全绿)
# 4. 交付: taskkill //IM "SorahkDFO-新UI版.exe" //F   (用户运行中会锁文件)
#    cp /e/Sorahk-build/target/release/sorahk.exe "E:\网页小工具\DfoVibration V3版本\SorahkDFO-新UI版.exe"
```
⚠ **交付纪律**: 构建/测试失败时**绝对不要 cp**(管道 `| tail` 会掩码退出码, 曾两次把旧包当新包交付)。cp 前用 `grep -ac "新功能标记" exe` 验证二进制含新代码。

### 验证手段 (实测结论)
- **离屏截图 ✓**: 窗口移到 (2600,100) 屏外 → PowerShell PrintWindow(PW_RENDERFULLCONTENT=2) 抓帧 → 完整渲染, 全程不扰用户。工具脚本模式在 HANDOFF 历史里, 按需重写 (用户在用电脑时用此法)
- **交互验证 ✗ 不可后台化**: egui-winit 忽略未聚焦窗口的 PostMessage 鼠标点击; SendInput 真实点击与用户鼠标冲突 → 按钮级验证只能**请用户手测** (给清单) 或前台自动化 (需用户允许)
- **遮挡白屏 (上游 bug)**: 启动即被全屏遮挡的实例永不呈现首帧, 永远白屏; 任务栏点击一次即恢复。与代码无关 (旧版同样复现, 已对照实验证明)。规避: 启动时保证窗口可见, 或让用户点一下

## 四、与用户协作的注意事项

- 用户以**截图 + 红框**方式反馈视觉问题, 常为裁剪缩放图 (坐标不可信), 需自行离屏复测精确坐标
- 用户说「对齐」可能指水平或垂直——**画中线/描框调试** (`painter().line_segment/rect_stroke` 临时插入, 离屏截图目测) 是最高效的定位手段, 用户也认可此法
- 用户在用电脑时: 只做离屏验证, 不要抢焦点/前置窗口/移动他们的鼠标
- 微调类需求 (±1px) 直接改代码里的常量 (如 `class_nudge`), 改完交付让用户看
- **直接 Edit 源文件, 不要写 Python 补丁脚本** (脚本叠加曾导致括号错乱/重复字段/失败仍交付, 教训见坑清单末条)

## 五、踩过的坑 (别再踩, 全部实测)

### egui 布局
- **`ui.horizontal()` 是 18px 条带**: 子 Ui 初始高度 = `interact_size.y` 且顶在内容区顶部, 直接放入的控件以条带中心居中, 嵌套容器又各有基准 → 顶栏曾出现四种垂直中心。**标题栏用 `ui.horizontal_centered()`** (占满整条高度再垂直居中, 官方为此设计); 勿嵌在垂直流中间
- **`main_align` 不作用于水平行的摆放** (0.33): 非 wrap 水平布局组件一律贴边放, `with_main_align(Center)` 只影响按钮/Frame 内部文字对齐。行级居中 = 测内容宽 → `allocate_ui_with_layout(定宽, …)` 让 `top_down(Center)` 父级居中; **间隙要算两截: add_space + item_spacing**
- **ComboBox 内嵌 horizontal 且顶部锚定**: 放进居中的行会下沉 (combo_h−18)/2; 同行两控件垂直错位 ~3.6px。修法: 等高显式槽位包裹 (极简全职业两级 combo 即此写法), 或按 combo.rect 用 `ui.put` 反推 label
- **`horizontal_centered` 是 centered_and_justified**: 会垂直占满剩余空间, 嵌套在垂直流中间会把后续内容挤出视口
- **top_down 父级下 `add_space` 是垂直空距**, 不能用来水平定位
- **hit-test 平局**: 同层两可交互区距离同为 0 → 后注册者胜; "薄者优先"只对不被完全包含的薄条生效。**大拖动区必须先注册(垫底), 按钮后注册**, 反了按钮全灭 (`buttons_on_window` 测试即此语义)

### 渲染
- **裸 `egui::Mesh` 会整帧白屏**: Mesh::default 的 texture_id = Invalid。渐变用分层半透明矩形 (`widgets::paint_top_glow`)
- **遮挡白屏**: 见"验证手段"; keepalive 线程 (500ms request_repaint) 只缓解不根治
- **真粗体**: `strong()` 只是变色。Bold 字体族在 fonts.rs (segoeuib + msyhbd.ttc + phosphor + Segoe UI Symbol/Emoji 回退), 用 `Theme::font_bold()`; 族缺 emoji/符号回退会渲染成 "?"
- **中英混排基线错位**: Latin=内置字体, CJK 回退雅黑, 度量不同 → 相邻不同字号 label 各自居中会错位; 用 `LayoutJob` 单 galley 共享基线
- **egui-phosphor**: 0.11 跟随 egui 0.33 (0.13 锁 0.35); 常量在 `variants::regular`

### 配置/持久化
- **`save_to_file` 是手工 format! 模板**, 不是 serde 序列化 — **新增 Config 字段必须同步模板行 + format 参数**, 否则字段静默丢失 (guide_seen 曾中招); Option 字段用条件行 (None 不写)
- serde(default) 字段: 旧配置缺字段时取默认 ✓; 但手工模板不写 = 每次启动都是默认值
- **ViewportInfo 的 inner_rect/outer_rect 已是逻辑坐标** — 不要再除以 ppp (双重换算曾致位置记忆缩小 25%)
- **OuterPosition 与 InnerSize 同批会竞争**: 位置可能被吞 → 进入极简后 12 帧位置锁存持续校正 (`minimal_pos_latch`)

### 其他
- `strong()` = 变色非加粗; `truncate_chars` 防中文按字节切片 panic; 保存失败静默 (`let _ =`) 注意
- Area 无 fixed_rect; ViewportCommand 是 `BeginResize` 不是 `StartResize`
- **工作流教训**: Edit 后文件状态失效需重 Read; 失败的构建绝不能交付; 多次叠加的行级补丁会自我冲突 — 大改用整体重写

## 六、架构速览

```
src/gui/
├── theme.rs           设计系统 v3.1 唯一来源: 色板(深浅)/字号/间距/圆角 + 卡片/徽章/按钮构建器
│                        (⚠ 强调色等微调常量: class_nudge 在 main_window.rs 职业槽)
├── widgets.rs         组件库: Icon(Phosphor)/nav_item/keycap/status_dot/paint_top_glow
├── fonts.rs           字体: 默认族(Ubuntu+雅黑回退) + "Bold"族(segoeuib+msyhbd+phosphor+emoji回退)
├── types.rs           Page 枚举 + KeyCaptureMode
├── mod.rs             SorahkGui 状态容器 (含极简/矩形记忆/分段状态字段) + 样式缓存 + run()
├── main_window.rs     App Shell + 连发页 + 预设管理卡 + 振动页 + 极简模式 + 自绘标题栏 + 首次指引
├── gamepad_mapping.rs 手柄页 (SVG 热点标定 — 坐标与 viewBox 绑定, 勿动)
└── *_dialog.rs        6 弹窗 (色板已迁移至 theme)
resources/gamepad.svg  无文字手柄图
design.md              设计规范 (新页面必读, 与 theme.rs 同步维护)
```

**关键状态字段** (SorahkGui): `minimal_mode` / `minimal_window_rect`·`normal_window_rect` (位置记忆) / `minimal_pos_latch` (位置锁存帧数) / `minimal_vib_preset_job` (极简预设来源, 持久化) / `preset_combo_h` / `minimal_job_block_w` (跨帧实测宽) / `page_preset_*` (预设管理卡状态)

**预设应用方法** (闭包内用 _in 显式参数版, 避免 E0502 整体捕获):
`apply_general_vibration_preset_in / disable_job_vibration_preset_in / apply_job_vibration_preset_in` (+ &mut self 包装版)

## 七、遗留事项 (P2, 不影响功能)

1. device_manager/mouse_* 少量旧灰阶字面量、部分对话框圆角 14/10 未归一
2. gamepad 热点色为 SVG 对比度特调 (故意保留, 勿"修复")
3. badge_clickable 以 rect 位作 Id, 重叠徽章会冲突
4. 手柄热点坐标与 SVG viewBox 绑定 (22 135 545 401), 改图即错位
5. 无边框窗口无系统圆角/阴影 (Win11) — 用户知晓
6. 可选: 小窗常驻置顶 / 完整 i18n / wgpu 圆角

## 八、用户手测清单 (最新一轮, 等用户反馈)

① 首次运行指引 (新配置才弹) → 开始使用 → 不再弹
② 极简 100% 小窗 (多次切换验证)
③ 全职业预设: 基础职业/转职两级选择即点即用; **「鬼剑士」「剑魂」同一水平线 + 行居中** (本轮核心修复)
④ 连发映射页预设管理卡: 保存/切换/重命名/删除双确认
⑤ 设置单齿轮; ⑥ 极简/完整来回切换窗口位置各回各位; ⑦ 剑魂 1px 微调合意度
8. v3.5 (2026-09-07): 新增 DFO 玩家引导 — config.dfo_player (serde default true, 老用户不变); 首启弹窗询问「是否 DFO 玩家」→ false 时隐藏「通用震动」「全职业预设」侧边栏入口 + 极简震动段 (状态行显示已关闭), 设置勾选项 t.dfo_vibration_feature 可随时改; 标题栏新增「?」按钮 (show_guide) 随时重开使用说明; 使用说明重写 (4 步快速上手 + DFO 询问面板 + 国产手柄排查指引); XInput 多 DLL 探测 (1_4/1_3/9_1_0/xinput 顺序, 输入与震动同源, 兜底系统导出) — 国产手柄适配
9. 重构 Phase 1-3 (2026-09-07): main_window.rs 4797 行拆为 gui/{shell(留在 main_window),vibration_page,turbo_page,minimal,guide}; theme 新增 modal_window 构建器 (鼠标×2 弹窗已收编, 新弹窗一律用它); Cargo.toml 新增 [profile.iter] (日常迭代 cargo build --profile iter, 交付仍 --release)。后续大步骤方案: C1 把 ~50 个 vibration_* 原子收进 VibrationParams 深模块 (编译器驱动全量改名); C6 job_presets 14 文件 → include_str! TOML 数据驱动 (JobClass 结构不动); C7 i18n 178 个透传 getter 删除。均未做, 按 HANDOFF 此条即可续作。
10. 白名单独立页 (2026-09-08): Page::Whitelist (TABS 第 3 位, Icon::Shield=SHIELD_CHECK, DFO 门控之外恒可见); config.whitelist_enabled (serde default true) + AppState whitelist_enabled AtomicBool (is_process_whitelisted 首查开关, false=全部放行且列表保留); gui/whitelist_page.rs: 状态胶囊总开关 + 添加(防重名忽略大小写)/浏览(rfd 选 exe)/删除, 实时 save+reload_config; 与设置弹窗编辑同一份列表。首启弹窗已拆两段式: render_dfo_ask_window (第 1 弹) → render_guide_window (第 2 弹, 会话内 dfo_ask_answered 防重复)。
11. 图标 + 托盘加固 (2026-09-08): resources/sorahk.ico 换为主题紫 #6C7CFF 圆角方块 (纯 Python 生成 PNG 条目式 ICO, 16-256 七尺寸, 3.2KB); 托盘四条外部建议核实: V2 cbSize 与 NIM_ADD 检查早已存在、NIF_SHOWTIP 本就不在代码中, 仅采纳 restore_main_window (FindWindowW 按视口标题 "DfoVibration-V3 连发与映射工具" → ShowWindowAsync(SW_RESTORE) → SetForegroundWindow, 托盘双击/菜单"显示窗口"/"关于"三入口调用, 之后仍 request_show_window 双保险); sync_and_build.sh 补 resources/build.rs 同步 (此前漏拷导致图标改动不生效)。
12. 托盘最小化真修复 (2026-09-08): 根因 = 旧实现「最小化到托盘」发 Minimized(true) (最小化到任务栏), 而 winit set_minimized(false) 在 Win11 恢复常静默失效 (上游已知问题)。修复 = 改为 ViewportCommand::Visible(false) 真隐藏 (任务栏按钮消失), 恢复 = 托盘侧 Win32 直连 (restore_main_window) + update 轮询 Visible(true)/Minimized(false)/Focus 三连; FindWindowW 失败会写 TRAY_RESTORE 崩溃日志便于排查。端到端验证: SW_HIDE → 托盘同款 Win32 恢复调用 → 窗口可见且内容完整重绘。
13. 手柄图双主题 (2026-09-08): resources/gamepad-light.svg (亮色变体, 仅 <style> 颜色不同, 几何/viewBox 与 dark 版完全一致 — 热点坐标共用); load_gamepad_texture(ctx, dark) 双变体嵌入, 纹理按主题缓存 (SorahkGui.gamepad_texture_dark 标记, 切主题自动重渲染)。改手柄颜色时两个文件都要改, 且只许动 <style>, 不许动几何。
