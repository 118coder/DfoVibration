# DfoVibration-V3 (原 SorahkDFO) — 交接文档 (HANDOFF)

> 更新: 2026-09-07 · 目的: 让下一个对话/会话无需翻历史即可无缝接手
> 配套: `design.md`(设计规范, 新页面必读) · `docs/DESIGN.md` · `REFACTOR_PLAN.md`(历史日志)

---

## 一、项目是什么

**DfoVibration-V3** (内部标识仍为 Sorahk/SorahkGui, 未改): 基于 [Sorahk](https://github.com/llnut/Sorahk) 的 DNF 游戏工具:
- **连发**: 按键映射 (键盘/鼠标/手柄触发 → 目标键), 多线程涡轮连发
- **震动**: DLL 采集游戏战斗事件 → Xbox 手柄马达, 参数可调
- 技术栈: **Rust + egui 0.33.3 + eframe(glow) + windows-rs**, 单文件 exe

仓库布局 (2026-09-08 拆分为三个独立 git 仓库, 父目录是纯运行时目录):
- `SorahkDFO源码\` — **主程序仓库** (本目录): 全部源码 + docs 全套文档 + UI部件
- `DLL源码\` — **DfoVibration-DLL 仓库**: 采集 DLL 源码 (含 vib_protocol.h ABI 契约)
- `研究文档\` — **逆向研究仓库**: IDA 报告/分析合集/会话记录/00总览(接手必读)
- `SorahkDFO-新UI版.exe` — **最新交付版** (显示名 DfoVibration-V3)
- `Config.toml` / `Vibration.toml` / `JobVibration.toml` — 用户配置 (App 自管理, serde 全字段落盘)

构建目录: `E:\Sorahk-build\` (中文路径 MinGW 链接会失败, 必须用此 ASCII 目录)。
日常快速自测: `cargo build --profile iter` (无 LTO, 增量); **交付一律 `--release`**。

## 二、当前状态 (2026-09-08)

✅ **420→428 测试全绿** (+4 双路线回归) · 最新 exe 已交付 (2026-09-08 深夜: S1/S4 双路线)
✅ **震动双路线 (2026-09-08 深夜, 用户拍板方案, 详见第八节 19 条)**: S1 ACT1 → 老方案 (老 DLL 事件语义配套:
   通道拆分/合成式输出/量纲归一/重映射前置), S4+ 新版 → 现行新方案 (默认, 逐字节不变)。
   首启 DFO 询问选「是」后追加版本询问弹窗; 设置「DFO 震动功能」处可随时切换 (勾选 DFO 玩家才可用)
✅ **潜藏 bug 修复轮 (2026-09-08 晚, 详见第八节 18 条)**: avx2 转义 UB+乱码 / 震动导入值无上限 两处真修复;
   新发现 params[39] 引擎槽位冲突 (需 schema 扩容, P1 待办); 旧记录「ownership 竞态」细节已随旧版 HANDOFF 丢失
⚠ **弹窗收编已回退 (fdc0725 revert 53e4d0d, 用户决定)**: 四窗迁到 modal_window 后全部塌成点状小窗。
   **modal_window 构建器本身存在未解 bug** (见第八节 17 条调查记录), 收编遗留项冻结: 修复前不要再把任何弹窗迁进去;
   鼠标×2 弹窗同用此构建器, 疑似同样受影响, 未实测。
✅ 设计系统 **v4 "Violet Night"**: 深色 = 深靛夜空紫韵 (bg #0F0F23 / accent #8B5CF6);
   亮色 = **蓝白基调** (bg #F4F5F7 / accent #4F55E8, 用户明确偏好, 勿改紫)
✅ 手柄 SVG 双变体: 深色版黑机身+**白字母**, 亮色版白机身+**深灰字母**;
   **XABY 彩色字母为用户明确要求保留 (不准改!)**; 机身只允许黑白灰
✅ 设计规范: design.md (v3.2 按钮语义层级强制 + v4 配色说明)

### 今日全部成果 (按提交序, 均已验证):
53e4d0d 弹窗收编四窗 (**已回退 by fdc0725, 构建器点状 bug**) · 64fb370 XABY还原 ·
1820f15 main_window(4797行)拆五模块 · e7f927d modal构建器+iter profile ·
6fae285 v4配色 · ed5dbdd 亮色蓝白+手柄纯黑白 · c780794 热点三层重绘 ·
5f41dc2 手柄双主题 · 34d15ab 紫色圆点图标+托盘加固 · 78e1bac 重置真修复(xinput1_3) ·
9b5f1d8 捕获确认/取消 · db1418f Esc绑定 · 1c864f6 白名单页 ·
6930677 DFO引导/?按钮/多DLL · ff30725 托盘隐藏式修复 · 早期: serde配置迁移+卡键P0批次

### 功能清单 (全部已交付): 白名单独立页 / 两段式首启弹窗(DFO询问→使用说明) /
标题栏「?」/ 手柄快速捕获确认取消+Esc绑定 / XInput 多DLL探测+1_3真实重置 /
托盘隐藏式最小化+Win32恢复 / 手柄双主题+热点重绘 / 配置serde全字段落盘+roundtrip

### 遗留 (下一会话按此接):
1. **弹窗收编 (冻结中)**: 前置 = 先查明 modal_window 构建器点状塌缩根因 (见第八节 17 条), 否则不要动
2. **C1 VibrationParams**: ~50 个震动原子收拢为深模块 (触点多, 需独立会话)
3. **params[39] 槽位冲突 schema 扩容 (P1)**: P_DEC_EFFECT 与 P_MOVE_BOOST_WIN 共用 39, 需 params 60→61 (见第八节 18 条③)
4. C6 job_presets TOML 数据驱动 / C7 i18n 透传 getter 删除 / 双胞胎弹窗完全合并
5. 待实机验证: 重置蓝牙重连效果 / 国产手柄多DLL适配 / X→托盘→恢复全流程
6. 导入值上限 (已修 18 条②) / avx2 UB (已修 18 条①) / ownership 竞态 (记录丢失, 见 18 条④)

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
14. 手柄热点重绘 (2026-09-08): 热点从单层半透明圆改为「玻璃底 + 主环 + 中心点」三层结构; 状态语义: 选中=accent 环+柔光放大 / 已配置=类色 2.0 实线环+实心点 / 空槽=类色 1.4 细线+淡点 / 悬停=柔光+150ms 放大 / 捕获=雷达扩散环; 摇杆按下 (StickClick) 改为隐形热点 (不绘制, 保留命中优先); 文字带微投影且主题感知配色。所有绘制在 gamepad_mapping.rs render_gamepad_svg 内, 几何/命中逻辑未动。
15. 重置手柄真修复 (2026-09-08): 根因 = XInputEnable 在 xinput1_4/9_1_0 (Win8+) 是空操作, 多 DLL 探测选中 1_4 后「重置」退化成清缓存。修复 = xinput.rs 新增 xinput13_enable() (惰性加载 xinput1_3.dll 专取其真实 Enable) + handle_xinput_reset() (1_3 切断电源 → 150ms 断电窗口 → 恢复, 期间清缓存+补发 Released); 回归测试: 1_3 可解析 + 重置接线。
16. 手柄快速捕获确认/取消 (2026-09-08): 捕获不再立即写映射 — SorahkGui.quick_gamepad_pending (槽位 id, is_trigger, 输入名) 暂存捕获结果, 槽位面板顶部渲染「⚠ 待确认」条 (✓ 确认应用 / ✕ 取消, 确认才 save+reload_config); 新捕获开始/Esc 自动丢弃待确认; capture 处理函数 (settings_dialog.rs) 只暂存不再直接落盘。
17. 捕获 Esc 键支持 (2026-09-08): 物理 Esc = 取消捕获 (不变); 槽位面板捕获等待态新增「或直接绑定 Esc 键」按钮 → 结果进待确认条 (确认应用才生效)。ESC 键名在 string_to_vk 已支持 ("ESCAPE"|"ESC")。连发编辑行的 TextEdit 本就可手输键名, 此改动补齐了快速捕获 (无文本框) 这条路径。
16b. 亮色主题回调蓝白 + 手柄纯黑白 (2026-09-08 用户偏好): light() 恢复蓝白基调 (bg #F4F5F7 / accent #4F55E8 电光靛蓝) — 深浅两主题各自成立 (dark=Violet Night, light=蓝白); 手柄双 SVG 移除彩色功能键 (st9-12 字母: dark→#F5F5F5, light→#2B2B2B), 亮色机身蓝灰调全部转纯中性灰。规则: 手柄 SVG 只允许黑白灰。
17. **弹窗收编回退 + modal_window 点状 bug 调查记录 (2026-09-08, 未结案)**: 53e4d0d 把设置/设备/关于/关闭四窗迁到 theme::modal_window 后, 四窗全部塌成 ~24px 点状小窗 (用户实测 + 离屏复现均确认), 已 revert (fdc0725) 回内联样板。调查事实 (全部离屏实测, 复现工具 = work/offscreen_capture.ps1: 启动→移屏外(2600,100)→PrintWindow 抓帧): ① 点确认为经由构建器渲染的弹窗本身 (fill 换红点即变红); ② 二分排除法: id 加 `.with("modal_win")` 后缀 / 标题文本 / radius16+stroke NONE+shadow 三件套 —— 逐项加回内联链全部正常渲染, 与构建器逐字等价却一个点一个全尺寸; ③ 唯一未排除变量 = 「闭包经函数参数转发」(builder 内 `.show(ctx, |ui| add_contents)` vs 内联闭包); ④ 海森堡: 给构建器加每帧文件日志后点状消失, 日志显示此时 egui 布局完全健康 (rect 512×499 居中), 即点状 = 布局正常但绘制塌缩, 与首帧 sizing pass / Area state.size 卡死类机制吻合但未定案; ⑤ egui 源码事实: Window::fixed_size 只作用于内部 Resize 容器 (不设 area.default_size), 窗口可见尺寸实取 Resize::end 的 last_content_size; CollapsingState::show_body_unindented 在 openness≤0 时返回 None (内容不渲染); animate_bool 新 id 首调即返回目标值 1.0。⑥ 鼠标×2 弹窗同用此构建器, 自 e7f927d 起可能一直是点状, 无人开过未察觉。结论: 修复需独立会话专攻 (建议方向: 复现时对比两版的 egui Area state.size / sizing pass 时序), 修复前构建器禁用。
18. **潜藏 bug 修复轮 (2026-09-08 晚, 睡眠时段自主执行)**: 按 diagnosing-bugs 纪律「红灯测试→修复→绿灯」闭环:
    ① **tray.rs xml_escape_avx2 双 bug 修复**: 旧实现 32 字节数据块用 `from_utf8_unchecked` 整块 push (跨块汉字切出非法 UTF-8 = UB), 且尾部兜底按 `byte as char` 处理 (非 ASCII 全变 Latin-1 乱码)。修复 = SIMD 块加纯 ASCII 守卫 (`_mm256_movemask_epi8(chunk)` 高位检查, 混入非 ASCII 即断出), 尾部改 `xml_escape_scalar(&s[i..])` 字符级转义 (i 恒为字符边界)。回归测试 `xml_escape_fast_matches_scalar` (cfg avx2, tray.rs 在 **bin** crate 非 lib, 测试要 `cargo test --bin sorahk` 跑): 红灯确认 case2 跨块汉字不一致 → 修复后 8/8 绿 (RUSTFLAGS="-C target-feature=+avx2")。发布构建未开 avx2 = 此前该路径是死代码, 无线上影响。
    ② **震动导入值上限修复 (job_presets/mod.rs)**: parse_job_export / parse_vibration_export 此前只查数组长度不查数值范围, u32::MAX 级数值直送引擎, rank_lr 负数 `as u32` 回绕。新增 `clamp_params` (60 槽逐一按震动页滑块 hi 钳位, 未列出索引按 % 类 100) + `clamp_signed_u32` (±100 二补码), 两个解析器收口。回归测试 4 个 (越界钳位/合法值不误伤/二补码保留/长度校验) 全绿, 全量 420/0。同时修了一个编译错 (i32 引用上 clamp 需 `(*v)`)。
    ③ **新发现 P1 待办 — params[39] 引擎槽位冲突**: vibration.rs `P_DEC_EFFECT=39` (装备特效衰减) 与 `P_MOVE_BOOST_WIN=39` (移动积累增强窗口, v22 新增) 共用同一槽, 震动页两个滑块写同一位, 后动者覆盖前者; 0..59 无空闲位, 正解需 params 60→61 / advanced 41→42 schema 扩容 (牵动导出格式 len==60 校验 / 内置预设 / config 数组), 需专项会话。
    ④ **旧记录核实**: 「ownership 竞态」细节随旧版 HANDOFF 重写丢失 (全历史 blob 检索只剩一句话), 候选位置 = temp_config 字段级合并 / 原子双写, 无红灯判据不动手; state.rs CaptureMode from_str().unwrap() 为假警报 (Err=Infallible 永不失败); clippy 无 correctness 级新发现 (93 条全是 unused import / cast); rawinput from_raw_parts 越界防护已在 (v1.6 继承)。
19. **震动双路线: S1 ACT1 老方案 / S4+ 新方案 (2026-09-08 深夜, 用户拍板)**: 用户实测老宿主 (`E:\LX\DfoVibration_OLD`, 宿主源码 `C:\srchk_src`) 玩新版客户端"高攻群怪持续震"不爽, 要求兼容新老两客户端 —— 新版走新方案, 老版走老方案。实现 (全部老方案语义 gate 在 `engine.legacy`, 新方案代码路径逐字节不变):
    - **选择流**: 首启 DFO 询问选「是」→ 新增第 1.5 弹「你玩的是哪个版本的 DNF?」(guide.rs `render_edition_ask_window`) → 使用说明; 非 DFO 玩家跳过。设置「DFO 震动功能」新增"客户端版本 (震动方案)"切换按钮 (`add_enabled_ui` 门控 dfo_player)。config 新增 `vib_legacy_client` (默认 false=新方案) + `vib_edition_asked`; state 新增 `vib_legacy_client: AtomicBool` (reload_config 同步, 震动线程每轮读 → 切换即时生效); 设置合并清单补 `vib_edition_asked` (主窗口所有)。
    - **老方案语义 (自 `C:\srchk_src` v13.34/35/27/30 移植, 均用户在老客户端实机验证过)**: ① FONT 通道拆分: 0x60(0x20|0x40, 老 DLL stub10 CC tick)→mode1 hold / 纯 0x20 DOT→mode3 衰减脉冲 / 0x04 怪物 DOT→mode4 脉冲 (V3 的 DLL 现不发 0x40, 0x60 通道待 DLL 侧跟进, 纯 0x20 拆分立即可用); ② update() 合成式输出 (hold/rhythm/decay 三分量取 max, inject 不清共存通道); ③ FONT /100 量纲归一 (+Burst /100 +删 dot_active 0.03 保底); ④ finalize 重映射前置+死区只杀真零 (hi=阈值); ⑤ 分通道注入窗 last_font_ch[4] (补丁A); ⑥ vib_log 诊断日志 ([INJ]/[DROP]/[MV] → exe 同目录 SorahkDFO_vib.log, 老方案专属)。
    - **防倒退红线 (移植时保住的 V3 独有优势)**: before()/wrapping_add 回绕安全、VIB_SHM_VERSION 握手、共享内存环校验、send_vibration 多槽扫描 —— 老宿主均无。
    - **战斗事件统计不动** (用户明确要求"战斗事件不要删掉, 这是要记录的"): 计数在环读取层 (push_event 之外逐条 fetch_add), 与引擎路线无关, 两路线口径一致。
    - **回归测试** (vibration.rs `legacy_route_tests`, bin 目标 4 个): 老方案 finalize 救起 25% 轻反馈 (≥28%)/新方案死区仍归零/老方案 inject 不打断 hold/finalize 两路线同构。全量 428/0。弹窗已离屏截图验证。
    - **验收口径**: 新方案 = 90US 玩起来与升级前完全一致 (默认, 理论零变化); 老方案 = 配老 DLL 在 ACT1 客户端复现 v7 宿主手感。⚠ 老方案在 90US 上 = 用户不爽的"持续震"属预期 (那正是老方案语义), 别当 bug 报。
