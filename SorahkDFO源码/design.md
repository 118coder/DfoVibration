# DfoVibration-V3 设计规范 (Design System v3.1 — "Obsidian Console" Vivid)

> 生效日期: 2026-09-06 (v3.1 当晚二次迭代: 更鲜明色板/改名/极简震动预设/位置记忆) · 维护文件: `src/gui/theme.rs` (唯一令牌来源) + `src/gui/widgets.rs` (组件库) + `src/gui/fonts.rs` (字体)
> 设计参照: Linear(页面 CSS 实测令牌) / Raycast / Vercel Geist / Radix Colors / Material 3
> **任何新页面/新组件, 动手前必须先读本文件, 并严格按规范实现; 一切颜色/字号/间距/圆角从 `theme.rs` 取, 禁止在业务代码写死字面量。**

---

## 1. 设计理念

1. **单一强调色**: 全 UI 只有一个电光靛蓝品牌色 — 深色 `#6C7CFF` / 浅色 `#4F55E8` (v3.1 应"更鲜明"需求提高饱和度)。强调色只出现在: 选中态、主按钮、激活滑块、状态点、左沿指示条、分段开关滑块。
2. **深色用表面色阶定层级, 不用阴影**: 相邻层亮度差 3-5%; 浅色用 1px 边框 + 柔和投影。
3. **背景带极轻蓝相** (slate 系而非纯灰), 深色内容区顶部有 accent 5% 微光渐变 (纵深暗示)。
4. **语义色低饱和**, 只标注状态, 不参与装饰; 彩色面积占比 <5%。
5. **对比度**: 正文 ≥4.5:1 (目标 6-7:1), 12px 小字 ≥4.5:1, 禁用/装饰 ≥3:1; 深色文字不用纯白 (用 #EDEEF0)。
6. **微动效**: 150ms ease-out, 只动透明度/颜色; 选中态 100ms。

---

## 2. 配色 (Color Tokens)

> 定义于 `theme.rs::Theme::dark() / light()`。以下 hex 均为最终生效值。

### 2.1 深色主题 (Obsidian)

| Token | 值 | 用途 |
|---|---|---|
| `bg` | `#0B0D11` | 应用底色 (内容区最深层) |
| `surface` | `#14161C` | 侧边栏 / 顶栏 / 弹窗底 |
| `card` | `#1A1D25` | 卡片面 |
| `card_alt` | `#21242E` | 卡片内嵌套面板 |
| `faint` | `#292D39` | 悬停底 / 次级按钮底 |
| `extreme` | `#08090D` | 输入框 / 键帽底 |
| `stroke` | `#FFFFFF16` (白 8.6%) | hairline 分隔线 / 卡片描边 |
| `stroke_strong` | `#FFFFFF32` (白 20%) | 悬停描边 / 浮层描边 |
| `accent` | `#6C7CFF` | 强调填充 (滑块/指示条/分段开关) |
| `accent_hover` | `#8B98FF` | 强调悬停 |
| `accent_soft` | `#6C7CFF` @18% (alpha 46) | 选中软底 |
| `accent_text` | `#A8B2FF` | 深底上的强调文字/链接 |
| `title` | `#F0F2F7` | 一级标题 |
| `heading` | `#DCE0EA` | 二级标题 |
| `text` | `#C0C5D4` | 正文 |
| `text_weak` | `#9AA2B8` | 次要文字 |
| `hint` | `#828AA0` | 提示文字 (card 上 ≥4.9:1) |
| `good` | `#3DD68C` | 成功/运行中 |
| `bad` | `#E5484D` | 危险/错误 |
| `warn` | `#FFB224` | 警告/暂停 |
| `info` | `#4CC4E6` | 信息 |
| `btn_primary` | `#5A64F2` | 主按钮 (白字 4.9:1, 电光感) |
| `btn_danger` | `#C43C48` | 危险按钮 |
| `trigger_fg/bg` | `#FCD34D` @9% | 触发键徽章 (琥珀) |
| `target_fg/bg` | `#7DD3FC` @9% | 目标键徽章 (天蓝) |
| `motor_l` | `#FB923C` | 左马达 |
| `motor_r` | `#F87171` | 右马达 |

### 2.2 浅色主题 (Porcelain)

| Token | 值 | 用途 |
|---|---|---|
| `bg` | `#F4F5F7` | 应用底色 |
| `surface` | `#ECEEF1` | 侧边栏 / 顶栏 / 弹窗底 |
| `card` | `#FFFFFF` | 卡片面 |
| `card_alt` | `#F6F7F9` | 嵌套面板 |
| `faint` | `#E7E9EE` | 悬停底 / 次级按钮底 |
| `extreme` | `#FFFFFF` | 输入框 / 键帽底 |
| `stroke` | `#E2E4E7` | hairline / 卡片描边 (Linear 实测) |
| `stroke_strong` | `#D0D5E0` | 悬停描边 |
| `accent` | `#4F55E8` | 品牌色浅色档 (更鲜明) |
| `accent_hover` | `#5B62F2` | 强调悬停 |
| `accent_soft` | `#4F55E8` @10% | 选中软底 |
| `accent_text` | `#3F45D0` | 白底强调文字 (6.4:1) |
| `title` | `#10131C` | 一级标题 |
| `heading` | `#23283A` | 二级标题 |
| `text` | `#3E4453` | 正文 |
| `text_weak` | `#626A7C` | 次要文字 |
| `hint` | `#6B7386` | 提示文字 (白底 4.7:1) |
| `good` | `#16A34A` · `bad` | `#E11D48` · `warn` `#D97706` · `info` `#0284C7` |
| `btn_primary` | `#4F55E8` | 主按钮 (白字 5.2:1) |
| `btn_secondary` | `#E7E9EE` | 次级按钮底 (= faint) |
| `btn_secondary_text` | `#4A5060` | 次级按钮文字 (= text) |
| `btn_danger` | `#E11D48` | 危险按钮 |
| `good_soft` | `#16A34A` @9% | 成功徽章底 |
| `bad_soft` | `#E11D48` @8% | 危险徽章底 |
| `warn_soft` | `#D97706` @9% | 警告徽章底 |
| `trigger_fg/bg` | `#B45309` / `#FBBF24` @16% | 触发键徽章 (琥珀) |
| `target_fg/bg` | `#0365A1` / `#38BDF8` @16% | 目标键徽章 (天蓝) |
| `motor_l` | `#EA580C` | 左马达 |
| `motor_r` | `#DC2626` | 右马达 |

规则: 语义色 (good/bad/warn/info) 浅色用 -600 档、深色用亮档; 徽章底 = 同色 9-10% alpha。

---

## 3. 字体 (Typography)

### 3.1 字体族
- **默认族 (Proportional)**: egui 内置 Ubuntu-Light (拉丁) → Phosphor (图标) → Microsoft YaHei `msyh.ttc` (中文) → SimHei/SimSun (回退) → Segoe UI Emoji/Symbol (emoji)
- **粗体族 (Bold)**: Segoe UI Bold `segoeuib.ttf` (拉丁) → Phosphor → 雅黑 Bold `msyhbd.ttc` (中文)
  - ⚠ egui 的 `RichText::strong()` 只是变色不是加粗; **真粗体必须 `.family(Theme::font_bold())`**
- 语言切换时由 `fonts.rs::get_font_configs_for_language` 调整 CJK 优先级 (日语 BIZ UDGothic 等)

### 3.2 字号阶梯 (行高 ≈1.3, egui 自动)

| 层级 | 字号 | 字重 | 颜色 token | API |
|---|---|---|---|---|
| Display (页面大标题) | 22px | **Bold** | `title` | `th.h1()` |
| Title (卡片标题) | 16px | **Bold** | `heading` | `th.h2()` |
| Heading (分组小标题) | 12px | **Bold** | `text_weak` | `th.h3()` |
| Body (正文) | 13px | Regular | `text` | `th.body()` |
| Secondary (次要正文) | 12px | Regular | `text_weak` | `th.weak()` |
| Caption (提示/说明) | 12px | Regular | `hint` | `th.hint_text()` |
| 数值统计 (stat) | 18px | **Bold** | 语义色 (调用方传 good/bad/warn/info token) | `widgets::stat` |

⚠ 全局 `TextStyle::Heading` 被定义为 18px 但**已弃用** — 标题一律走 `th.h1/h2/h3` (自动 Bold 族 + 正确颜色), 不要用 `TextStyle::Heading`/`RichText::heading()`。

规则: 中文正文 ≥13px (egui 无 hinting, 12px 雅黑起读); 标题用 Bold 族 + `title/heading` 色; 一级标题与描述间 `SP_XS`。

---

## 4. 间距 (Spacing, 8pt 网格)

| Token | 值 | 用途 |
|---|---|---|
| `SP_XS` | 4 | 图标↔文字、标签↔控件 |
| `SP_S` | 8 | 默认 item_spacing、按钮组间距 |
| `SP_M` | 12 | 卡片之间的间距、嵌套面板内距 |
| `SP_L` | 16 | 卡片内边距、标题与内容距离 |
| `SP_XL` | 20 | 大区块间隔 |
| `SP_2XL` | 24 | 页面级留白 / 空状态上下 |

布局常量: 顶栏高 = 内容 32 + 上下边距 9 (margin 14,9); 侧边栏宽 188 (内距 10); 内容区边距 16; 卡片内边距 16; 嵌套面板内边距 12。全局 `button_padding = (12,5)`、`item_spacing = 8`、菜单圆角 10 (build_style 注入)。

---

## 5. 圆角 (Corner Radius)

| Token | 值 | 用途 |
|---|---|---|
| `RADIUS_CARD` | 12 | 卡片 / 弹窗窗口 |
| `RADIUS_NAV` | 10 | 侧边栏导航项 |
| `RADIUS_CTRL` | 8 | 一切按钮 / 输入框 / 图标按钮 |
| `RADIUS_CHIP` | 6 | 小元素 / 标题栏窗口按钮 |
| 胶囊 | 100 | 徽章 / 状态胶囊 (pill) |

同心圆角规则: 内层圆角 ≈ 外层圆角 − 内边距 (卡片 12 → 内嵌面板 8)。

---

## 6. 阴影与海拔 (Elevation)

**深色主题: 零卡片阴影。** 层级 = 表面色阶 (bg→surface→card→card_alt) + 白 8% 描边。浮层 (popup/menu) 保留深色投影 + 描边:
- window_shadow: offset `[0,14]` blur 30 `#00000040`
- popup_shadow: offset `[0,6]` blur 20 `#00000030`

**浅色主题: 卡片柔影** (M3 ambient 换算):
- 卡片: offset `[0,2]` blur 6 `#00000012`
- window_shadow: offset `[0,14]` blur 30 `#0000001A`; popup: `[0,6]` blur 20 `#00000014`

**顶部微光 (仅深色)**: 内容区顶部 260px, accent 5%→0 分层渐变 (`widgets::paint_top_glow`, 用分层半透明矩形实现 — 不要用裸 Mesh, 其默认 texture 为 Invalid 会白屏)。

键帽立体感: `extreme` 底 + 1px `stroke_strong` + 底部硬投影 (dark α110 / light α40, blur 0)。

---

## 7. 组件规范 (Components)

### 7.1 按钮 (theme.rs)
| 类型 | 底色 | 文字 | 圆角 | 高度 |
|---|---|---|---|---|
| Primary `primary_button` | `btn_primary` | 白 13px Bold-色 | 8 | auto |
| Secondary `secondary_button` | `faint` | `btn_secondary_text` | 8 | auto |
| Danger `danger_button` | `btn_danger` | 白 | 8 | auto |
| Ghost `ghost_button` | `accent_soft` | `accent` 12px | 8 | auto |
| 状态胶囊 `status_pill` | 语义 soft 色 | 语义色 12.5px | 12 | 28 |

标题栏窗口按钮: 34×26 / 圆角 6 / 透明底 → 悬停 150ms 渐入 `faint` (危险钮悬停 `bad`), 字形白色随 hover_t 淡入淡出。

### 7.2 卡片 (theme.rs)
- `card()`: 填充 `card` + 1px `stroke` + 圆角 12 + 内距 16 (+浅色柔影), 标题行 = h2 + 右侧动作区, 标题后 `SP_L`
- `panel()`: 嵌套二级容器 — `card_alt` + 1px stroke + 圆角 8 + 内距 12

### 7.3 导航 (widgets::nav_item)
- 尺寸: 可用宽 × 34px, 圆角 10; 图标 18px @left+24; 文字 13px @left+42
- 选中: accent 14% 软底 (100ms 动画) + 左沿 3×18px accent 指示条 + 图标/文字同 `accent/accent_text`
- 悬停: `stroke` 底 + 文字提亮; 焦点环: 外扩 2px 2px accent

### 7.4 徽章 (theme.rs)
- 徽章 = pill (圆角 100) + 内距 9×3 + 12px Bold-色文字; 语义底 = 同色低透明底 (alpha 原始值 20-42, ≈8-16%)
- 可点击徽章 `badge_clickable` 必须用它 (自带 interact), 别用裸 Frame

### 7.5 其他
- 键帽 `widgets::keycap`: 见 §6 键帽立体感
- 图标按钮 `widgets::icon_button`: 32×32 命中 / 18px 图标 / 圆角 8 / 悬停 150ms 渐入 + 焦点环
- 状态点 `widgets::status_dot`: 0.6s 呼吸动画 + 33ms 重绘请求 (防遮挡冻结)
- 图标: 一律 Phosphor (`widgets::Icon`), 禁止 emoji; 图标细节孔洞用 `color.gamma_multiply(0.4)`

### 7.6 对话框
- 浮窗: `surface` 底 (深) / 白 (浅), 圆角 12, 标题行 = 图标 + h2, 关闭 ✕ 右上
- 底部动作区: 主按钮 (保存/确认) + 取消; 危险操作用 danger
- 语义: 设置类弹窗打开时自动暂停连发 (`was_paused_before_settings`)

---

## 8. 布局 (Layout)

```
┌──────────────────────────────────────────────┐
│ 顶栏 14,9: 品牌方块+DfoVibration-V3(Bold)+页面标签  预设▾ ⚙设置 简 │ 设备 ⓘ 主题 ─ □ ✕ │
├──────────┬───────────────────────────────────┤
│ 导航 188  │  内容区 (bg + 顶部微光)             │
│ 内距10    │  边距16: page_header(h1+hint)      │
│ 导航项34  │  卡片(SPM 间隔) …                   │
│ 底部状态  │                                    │
└──────────┴───────────────────────────────────┘
```
- 软件名: **DfoVibration-V3**; 窗口/任务栏标题 = "DfoVibration-V3 连发与映射工具"
- 页面 = 枚举 `types.rs::Page`; 新页面 = 加枚举 + 导航项 + CentralPanel 分发 + ScrollArea
- 极简模式窗口 430×470 (逻辑): 顶栏 (品牌 DfoVibration-V3 / 完整界面 / 主题 / 窗口钮) + 居中列: 连发大开关 260×50 圆角 12 → 「连发与映射」预设行 → 震动大开关 → **震动预设分段开关 [通用预设|全职业预设]** (滑块 150ms 动画; 应用层互斥: 应用任一侧自动关另一侧) + 对应预设下拉 → 状态行
- **窗口位置记忆**: 进入极简/完整各自记忆窗口矩形, 切换时精确还原 (各回各位); 字段 `normal_window_rect` / `minimal_window_rect`
- 居中行规范: `top_down(Center)` 父级 + 定宽子块 (`allocate_ui_with_layout`); 注意 `ui.horizontal()` 只有 18px 高且不水平居中; **top_down 下 `add_space` 是垂直空距, 不能水平定位**; ComboBox 需等高包裹 (`render_preset_switch_centered`)

---

## 9. 动效 (Motion)

| 场景 | 时长 | 实现 |
|---|---|---|
| 导航选中 | 100ms | `animate_bool_with_time` (nav_item) |
| 按钮/图标悬停 | 150ms | `animate_bool_with_time` (icon_button / 窗口钮) |
| 状态点呼吸 | 0.6s 循环 | `animate_value_with_time` + 33ms 重绘 |
| 规则 | — | 只动颜色/透明度; egui 无布局过渡, 位置尺寸瞬时; 动画 Id 必须稳定 (rect 位派生) |

---

## 10. 代码约定 (给未来的页面开发)

1. **先读本文件**; 所有取色/字号/间距/圆角从 `Theme` / `theme::SP_*` / `theme::RADIUS_*` 取。
2. 新组件放 `widgets.rs`; 新文本样式放 `theme.rs`; 不允许业务文件出现 `Color32::from_rgb` 字面量 (语义色同样走 token)。
3. 文本: 标题用 `th.h1/h2/h3` (自带 Bold 族); 正文 `th.body/weak/hint_text`。
4. 深浅双主题必须同时成立: 颜色永远从 `self.theme()` 取, 禁止 `if dark` 手调 (除非语义色徽章)。
5. 自绘可点击控件必须保留: `Sense::click()` + `on_hover_cursor(PointingHand)` + 焦点环 + 稳定 Id。
6. 大面积自绘前先查 §6 与 HANDOFF「踩过的坑」(hit-test 平局、18px 条带、combo 内嵌条带、Mesh texture)。

---

*v3.1 "Obsidian Console Vivid" — 2026-09-06。变更记录: v2.1 蓝调深灰+indigo-400/600 分色 → v3.0 Linear 实证 #5E6AD2 单品牌色、表面层级加深、真粗体、柔影/微光、悬停动效、对话框色板迁移 → v3.1 应用户反馈: 强调色提饱和 (#6C7CFF/#4F55E8)、文字对比加深、软件改名 DfoVibration-V3、极简「连发与映射」标签、震动预设分段开关 (通用/全职业互斥)、窗口位置记忆。*
