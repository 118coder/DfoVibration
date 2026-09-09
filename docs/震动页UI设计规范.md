# SorahkDFO 震动页 UI 设计规范（防再犯备忘）

> 日期: 2026-08-13
> 背景: 震动设置页 UI 多次返工, 总结规范避免重复踩坑。

## 一、卡片（框框）规范 —— 与连发映射页完全一致

连发映射页卡片结构（**唯一正确参考**）:
```rust
egui::Frame::NONE
    .fill(if dark { Color32::from_rgb(40, 42, 50) } else { Color32::from_rgb(245, 238, 252) })
    .corner_radius(egui::CornerRadius::same(15))
    .inner_margin(egui::Margin::same(16))
    .show(ui, |ui| {
        ui.set_min_width(ui.available_width());   // 全宽, 必须!
        ui.label(RichText::new(标题).size(16.0).strong()
            .color(if dark { rgb(200,180,255) } else { rgb(150,100,200) }));
        ui.add_space(8.0);
        // 内容...
    });
```

**铁律**:
- 背景色: 暗 `(40,42,50)` / 亮 `(245,238,252)` —— 不许自定义其他背景
- 标题: 16px 粗体紫色 `(200,180,255)/(150,100,200)` —— 不许五颜六色标题
- **无描边**（stroke 只在弹窗强调时用, 如关闭对话框黄色边框）
- `set_min_width(available_width())` 全宽 —— 不许限制 max_width
- 圆角 15、内边距 16、卡片间距 10

## 二、布局铁律

1. **不用 Grid 做滑块布局** —— Grid 列宽由内容撑开会导致框边缘不统一。
   正确: 每项 = 独立 `ui.vertical` 块, `ui.set_width(ui.available_width())`,
   内部: 名称 label → 滑块 → 说明 → L/R 行。
2. **长文本自动换行** —— 用 `ui.add(egui::Label::new(RichText...).wrap())`,
   不要用 `ui.label`（默认不换行会撑爆卡片）。
3. **L/R 控件必须分行** —— L 一行（橙色）、R 一行（红色）, 不许挤同一行。
4. **不要设 `ui.set_width(w)` 在 horizontal 内部** —— 无效且撑宽。

## 三、控件与字号

- 震动页全局: `text_styles Body/Button = 14px`, `spacing.slider_width = 300`, `interact_size.y = 24`
- 项名称 label: 14px（深灰 `(200,200,200)` / 浅灰 `(40,40,40)`）
- 说明 hint: 11px weak
- L/R 标签: 13px strong（L=橙 `(255,140,0)` / R=红 `(255,80,80)`）
- 按钮: 13-14px

## 四、布局顺序（震动控制中心卡片内）

标题行(开关/测试) → 预设行(应用/保存当前/保存为/保存/删除/还原默认)
→ **"内置:" 说明独立一行**(wrap) → 提示行 → 分隔线 → 连接状态

## 五、参数存储备忘

- `item_lr` 是 `[u32; 26]`, 负权重用补码存。**源码里必须写 `lr(-10)`**（`const fn lr(w: i32) -> u32`），
  不许手写 `4294967286` 这种补码数字。
- 预设结构: `{ name, params: [u32;60], item_lr: [u32;26] }`, serde 用 vib_arr60/vib_arr26。
- 应用/保存/还原预设时**必须同步 item_lr**（3 处: 应用按钮、保存按钮、还原默认按钮）。

## 六、历史教训

- 补码数字被正则批量替换误伤过注释 → 只替换函数体内, 用行范围限制
- 预设默认值以 `G:\game\USDOF\Vibration.toml`（玩家实测版）为准, 同步进 config.rs 内置预设
- PowerShell 改 Rust 源码要小心 `.size(10.0)` 等批量替换伤及无关代码 → 限定行范围
