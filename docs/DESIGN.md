# Sorahk UI 设计系统指南 (DESIGN.md)

> 2026-09-06 UI 全面重构后生成。目标: 任何后续维护者 10 分钟内理解界面如何改。

## 一、文件地图

```
src/gui/
├── theme.rs            设计系统唯一来源: 色板/字号/间距/圆角 + 卡片/徽章/按钮构建器
├── widgets.rs          复合组件: 13 枚矢量图标 + 导航项/键帽/状态点/空状态/统计块/图标按钮
├── types.rs            Page 枚举 (页面=导航项, 新增页面只改这里) + KeyCaptureMode
├── mod.rs              SorahkGui 状态容器 + 样式缓存 + 应用启动
├── main_window.rs      App Shell (顶栏/侧边栏/内容分发) + 连发页 + 振动页/职业页
├── gamepad_mapping.rs  手柄可视化页 (SVG 热点标定 + 槽位面板 + 已配置总览)
└── *_dialog.rs         6 个弹窗 (全部通过 Theme::new(dark_mode) 取色)
resources/gamepad.svg   无文字手柄图 (viewBox "22 135 545 401")
docs/gamepad-original-with-text.svg.bak   原始带字版备份
```

## 二、核心规则

1. **颜色唯一来源是 theme.rs**。业务代码写 `th.good` 而不是 `from_rgb(...)`。
   新语义颜色: 在 `Theme` 加字段, 明暗两套同步赋值。
2. **页面新增三步**: `types.rs` 加 `Page` 变体 → 补 `TABS`/`label`/`hint`/`icon`
   → `main_window.rs` 的 `render_shell` 分发里加分支。侧边栏/顶栏自动生效。
3. **手柄热点重标定**: 改了 `gamepad.svg` 必须重新标定 `SLOTS`
   (方法: resvg 渲染 → PIL 叠加圆圈 → 目测对齐; 坐标为 viewBox 归一化值)。
4. **构建**: 源码目录含中文无法用 MinGW 链接, 一律 `sync_and_build.sh`
   同步到 `E:\Sorahk-build` 后构建。

## 三、设计语言速查

| Token | 暗色 | 亮色 |
|-------|------|------|
| bg / surface / card / card_alt | #0D0F13 / #151820 / #1C2029 / #252B37 | #F4F5F8 / #EEF0F4 / #FFF / #F6F7FA |
| accent (唯一强调色) | #818CF8 | #4F46E5 |
| text / weak / hint | 196,201,212 / 154,160,172 / #8A91A5 | 66,72,88 / 105,112,128 / #64748B |
| good / bad / warn / info | #3DD68C / #EC5D5E / #FFB224 / #4CCCE6 | green-600 / rose-600 / amber-600 / sky-600 |

- **字号**: Display 22 (页头) / Title 16 (卡片) / Body 13 / Caption 12
  (Caption 不用 11 — 微软雅黑 12px 起步才可读, egui 无 hinting)。
- **间距**: 4/8/12/16/20/24 (SP_*); 卡片内边距 16, 面板 12。
- **圆角族**: 卡片 12 / 控件 8 / 面板 8 / 徽章全圆。
- **强调色只用于**: 选中态、主按钮、激活滑块、状态点。语义色只标状态。

## 四、常用组件

```rust
let th = self.theme();                       // Theme::new(self.dark_mode)
th.card(ui, Some("标题"), |ui| { ... });      // 主卡片 (亮色自带投影)
th.panel(ui, Some("子标题"), |ui| { ... });   // 嵌套面板
th.primary_button("保存") / secondary / danger / ghost
th.badge(ui, "文本", fg, bg)                  // 展示徽章
th.badge_clickable(...)                       // 可点击徽章 (egui Frame 无点击感测, 勿用 badge().clicked())
th.trigger_badge / target_badge               // 触发键(琥珀)/目标键(天蓝)
widgets::nav_item(ui, &th, Icon::X, "标签", selected)   // 侧边栏项 (自带动画/焦点环)
widgets::keycap(ui, &th, "DELETE")            // 键帽
widgets::status_dot(ui, color, pulsing, r)    // 呼吸状态点 (自带重绘请求)
widgets::icon_button(ui, &th, Icon::Gear, "tip")
widgets::empty_state(ui, &th, Icon::X, "标题", &["行1", "行2"])
widgets::stat(ui, &th, "5", "映射条目", th.accent_text)
```

## 五、已知取舍

- **i18n**: 主窗口新外壳文案为中文 (原振动页本就是中文硬编码);
  设置弹窗等旧文案仍走 translations。恢复多语言入口:
  `types.rs Page::label/hint` + `main_window.rs` 连发页/侧边栏文案。
- **XInput 不支持 Guide 键**, 手柄页无 Xbox 徽标槽位。
- **Badge 点击**: 必须用 `badge_clickable` (Frame 自身只有 hover 感测)。
- **主题切换**: `ctx.set_style(build_style(dark))` 每帧应用, Style 明暗各缓存一份。
