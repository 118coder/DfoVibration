# 扩展食谱 (Cookbook) — 标准操作配方

> 配合 `docs/开发维护规范.md` 使用。每张配方 = 最短正确路径 + 会拦住你的守卫测试。
> 原则: **照配方走, 守卫测试替你把关**; 绕过配方 = 大概率踩历史病历。

---

## 配方 1: 加一个震动参数（60 槽具名参数）

1. `src/vibration/params.rs` 宏表 `for_each_named_param!` 加一行 `(N => 新字段名)`
   （N = 分配的槽位下标, 用 `docs/震动系统开发规范_v20.md` 对应章节确认空闲槽位）。
2. `src/config.rs` 的 `VibrationConfig` 加同名字段 + `default_vib_*` 工厂默认值。
3. 引擎要读: `params.rs` 加 `pub(super) const P_新名: usize = N;`。
4. `tests/architecture_tests.rs` 的 `EXPECTED_VIBRATION_KEYS` 加新键（不加快照测试红 = 提醒）。
5. GUI 滑块: `src/gui/vibration_page/page.rs` 对应分区方法里挂滑块（写原子 p[N] +
   config 字段; 说明文案补"调高/调低"句, 参考 `param_hint` 表）。
6. 验证: `cargo test --lib mapping_tests`（往返恒等）→ **`act_variants_tests` 校验和会变,
   这是预期**——确认是有意变更后按测试提示更新校验和常量 → 全量测试 → 逐页截图。

⚠ 槽位 19-59 = `config.advanced[i-19]` 位置段: 加"高级参数"不进宏表, 只在 advanced 数组
尾部追加 + 引擎加 P_* 常量（历史槽位语义见 params.rs 注释, 别复用死槽位）。

## 配方 2: 加一个 GUI 页面

1. `src/gui/types.rs`: `Page` 枚举加变体 + `TABS` 列表加项（含矢量图标）。
2. 新建 `src/gui/<页面>_page/` 目录: `mod.rs`（声明）+ `page.rs`（`impl SorahkGui` 渲染入口,
   `pub(in crate::gui) fn render_xxx_page`）——参照 `turbo_page/` 的目录形态。
3. `main_window.rs` 的 `render_shell` 页面分发处接线; 完整/极简双形态都要接
   （`classic_mode.rs` / `minimal.rs` 皮肤变体）。
4. 侧边栏 Rail: `main_window.rs` 导航区加项（图标用 Phosphor / 自绘, 禁止 emoji）。
5. 验证: `gui_dependency_allowlist` 会拦住对钩子/托盘等接线层的越界引用 → 全量测试 →
   明暗双主题逐页截图（`work/offscreen_capture.ps1`, 带 `SORAHK_NO_AUTO_INJECT=1`）。

## 配方 3: 加一个客户端形态（auto_inject）

1. `src/auto_inject.rs` 的 `CLIENTS` 表加条目（进程名 / 目标 DLL / 标签 / 路线门控）。
2. 确认路线归属（S1 老客户端 → OLD.dll 系; 新一代 → DfoVibration.dll）, 路线判断
   `vib_legacy_client` 联动。
3. `docs/使用说明.txt` 补对应场景说明（管理员权限/注入行为）。
4. 验证: `auto_inject` 相关单测 + **真机验证必须走无 manifest 开发构建 + `SORAHK_NO_AUTO_INJECT=1`
   先行, 真注入只在用户在场时做**（v24.20a 教训）。

## 配方 4: 加一个设置项

1. 选对文件: 连发/界面/白名单 → `AppConfig`(Config.toml); 震动 → `VibrationConfig`
   (Vibration.toml / Vibration-ACT.toml 路线各自一份)。
2. 字段 + serde 属性 + Default 工厂; **字段名 = 兼容契约**, 命名后不可改。
3. `tests/architecture_tests.rs` 对应 `EXPECTED_*_KEYS` 加键。
4. GUI 接线（设置弹窗分区方法或页面卡片）; 保存路径确认: 主配置 `save_config_only`,
   震动走 `save_vibration_*`——别把震动写进 Config.toml（v24.15 路线隔离教训）。

## 配方 5: 发版（用户确认稳定后）

1. 交付构建: `bash sync_and_build.sh --release` → 记录 exe md5。
2. 入库 `release_backup\`（命名 `DfoVibration-Sorahk_v<版本>_<主题>_<日期>.exe`）。
3. 覆盖 `DfoVibration V3版本\` 交付别名（先 `tasklist` 确认没在运行）。
4. CHANGELOG + HANDOFF（版本/日期/md5/测试基线）→ git tag（`git tag v<版本>`）。
5. 上传: 等口令 → 分支 + PR 流程（HANDOFF 第 42 条）。

## 配方 6: 重构/大改前的安全网

1. `python work/_r1_equiv_check.py` — 拆分/搬家前后跑, 多重集对账（--mutate 可验证
   校验器自身灵敏度）。
2. 拆分模式参照: `src/state/`（B 系列）、`src/gui/vibration_page/`（C 系列）的文件头
   都注明了来源行号区间与可见性等价规则。
3. 每步一 commit; 契约测试(`architecture_tests`)与冲突矩阵（HANDOFF 日志 90 清单）全绿才算完。
