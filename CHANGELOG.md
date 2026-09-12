v24.17 (2026-09-13)
===================
① 一刀多怪合并默认为开；② 所有参数说明补上"调高/调低"的明确后果（用户要求"高低有明确概念"）:

- **① 一刀多怪合并 (群怪一刀只震一下) = 开**:
  * 出厂默认本来就是开 (`default_vib_true`), 但用户现场配置里被关过 → 在 `act1_preset_extras`
    里写死 `merge_enabled = true`, 应用「ACT1 特供」后一定开。
  * 关掉 = 命中聚合窗/合并记账/补发全部回到旧行为 (一刀多怪各震各的、更吵)。
- **② 参数说明补"高低后果"**: 119 个可调控件里 112 个的悬浮说明都给出了
  "调高：…；调低：…"(开关类为"开：…；关：…")的明确后果; 余下 7 个是模式/分段按钮
  (调试中/正常模式/震动开/震动关/允许高级调校/显示专家参数/马达分工), 不是参数, 未加。
  * 原本**已有说明**的参数 → 在说明末尾追加高低句 (按唯一前缀定位, 逐条核对, 不覆盖原文);
  * 原本**只有名字**的参数 → 新增 `SorahkGui::param_hint(标签)` 单一说明表 + `with_hint()`,
    接线 8 个参数表循环 + 21 个独立滑块 + 5 个开关 (共 34 处)。
  * 自检: `work/_check_coverage.py` 列出没有高低说明的控件; 说明文本的事实来源 =
    `work/_apply_desc_hilo.py`。
- 测试: 新增 `merge_enabled_defaults_to_on` 回归锁; `act1_preset_extras` 测试纳入 merge_enabled
  (反向设 false 后必须被拉回 true); **703 通过 / 0 失败**。
- 实测: 临时把启动页改到「通用震动」跑离屏截图 → 整页正常渲染 (无 panic), 核对完立即还原
  (插桩未留在源码里)。
- exe md5 c7b25c85501ec552d34f039599468d85

v24.16 (2026-09-13)
===================
「ACT1 特供」预设按用户实机调优定稿（用户手调值，逐项落到预设表 + 附加默认）:

- **60 槽 / 评分族（预设本体现有字段）**:
  * 连击增强 (渐进至上限) `params[6]`: 35 → **0**（不做连击递增）
  * 玩家命中反馈 (0x01) `params[16]`: 65 → **36**
  * 评分点 `rank_type_gain[0]`: 20 → **25**
  * 释放技能 `rank_type_gain[9]`: 20 → **45**
  * 怪物死亡 `rank_type_gain[14]`: 45 → **40**
- **附加标量（不是 60 槽字段，走 `act1_preset_extras`，应用该预设时落地）**:
  * 打群怪几秒后自动降温 `sustain_secs`: 3 → **1 秒**（+ 母开关 `sustain_enabled=true`）
  * 震尾多快切断 `tail_land_pct`: 25 → **60**（+ 母开关 `tail_land_enabled=true`）
  * 统合衰减期 `storm_unified_ms`: 120 → **40**（+ `storm_unified_enabled=true`；该开关默认 false，
    不一起打开则滑块静默不生效）
  * 怪物异常反馈 (0x04) 出血/中毒/感电跳字 `monster_abnormal_gain`: 1 → **3**
  * GUI 应用预设处把这些字段一并同步到运行态原子量（原先只同步 3 个布尔开关）。
- **职业基底冻结（防静默连带）**: `act1_base_params()` 原为"读「ACT1 特供」预设的 params"，
  而 `act_build_params_from` 不重建 p[5]/[6]/[10]/[18]/[19]/[20] 等槽位 → 改这个通用预设会连带
  改掉全部 39 个职业 ACT 变体。改为 **v19.5 定稿快照**。
  验证: 39 个转职的 params+rank_type_gain 校验和改动前后逐字节一致（0xf96350194404a686）。
- 测试: 新增「预设表带用户手调值」+「职业基底是冻结快照（含剑魂-ACT 抽样）」两条；
  `act1_preset_extras` 测试扩到覆盖全部新标量（反向设置后必须被拉回）；**702 通过 / 0 失败**。
- 实测: 用户真实配置跑新版 → 持久化的「ACT1 特供」条目被自愈为定稿值
  (`params[6]=0 / params[16]=36 / rank_type_gain=[25,...,45,...,40]`)，39 个职业预设不变。
- exe md5 7ad5d0f5fbe7b7c511dec9dcd792fcb9

v24.15 (2026-09-13)
===================
路线隔离的三个漏口（自审 + 双轴 review 后用探针逐个复现并修掉；用户要求"再检查一下潜藏的 bug"）:

- **① 职业预设跨路线生效（最严重，直接架空"互不干预"）**: 启动恢复与震动页初始化都用
  `job_presets::find_class` —— 它**跨两条路线**查找，ACT 职业名 (`*-ACT`) 在 S4 路线也命中。
  探针实测: S4 路线 + `JobVibration.toml` 里是 S1 的 ACT 职业 → 启动参数被 ACT 职业覆盖
  (`params[0]` 77 → 11)，并随保存写进 **S4 的** `Vibration.toml`。
  修: 新增 `find_class_for_route(base, name, legacy_client)`（只认当前路线名册，返回该名册下标）;
  启动兜底 (state.rs) / 震动页恢复 / 职业档导入全部改走它; 路线切换时同步清掉
  `vib_job_enabled/active/loaded`（名册互不相交，切完必然不适用，否则界面还显示"已应用职业预设"）。
  JobVibration.toml 本身不改写 —— 切回原路线仍恢复原职业。
- **② 损坏的震动文件让程序完全起不来**: `load_or_create` 对解析错误直接 `?`
  → 与 v23.1 修的"坏触发键阻断启动"同类。新增 `load_vibration_tolerantly`: 坏文件挪成
  `<名>.bad` 留档、按出厂默认继续（数据不丢，可手工修回）。
- **③ 半截文件风险**: 保存是 `fs::write`（先截断再写），崩溃/断电/两个实例同时写会留半截文件
  → 配上 ② 就是"下次启动直接失败"。新增 `write_atomic`（同目录 `<名>.<pid>.<n>.tmp` + rename 覆盖）。
  临时名带 pid + 序号: 并行写多个配置（测试/多实例）互不踩踏。
- **④ 老 Config.toml 残留 `[vibration]` 段的跨路线通道**: 该字段原为 `skip_serializing` 但仍可**读**，
  路线文件缺失时旧段的值会当成该路线参数。改 `#[serde(skip)]`（读写都不经 Config.toml）,
  并在 `load_from_file` 里补回内置预设表（skip 后反序列化为空表会让预设下拉整个空掉）。
  另外路线文件缺失 = 首次使用 → 一律出厂默认，不继承任何残留值。
- 测试: 新增 `tests/route_isolation_tests.rs`（4 个场景: S4 不吃 ACT 职业 / S1 照常吃（防修过头）/
  残留段不继承 / 坏文件不阻断启动且挪 .bad）；**700 通过 / 0 失败**。
- 实测: 用用户真实配置（S1 路线 + 已迁移的 `Vibration-ACT.toml`）跑修复版 → 主窗口正常、
  预设「ACT1 特供」；TOML 深比较证明一次运行后**参数与 16 个预设逐字段不变**（含 60 槽/数组）。
- exe md5 f370d2d46cea85904165b0a5a630afcd

v24.14 (2026-09-13)
===================
震动设定改为「一条路线一个文件」(用户新方案 —— 彻底互不干预；替换 v24.13 的快照槽):

- **问题**: v24.13 用两个内存快照槽 (`vibration_saved_act` / `vibration_saved_new`) 存另一条路线的
  参数，两条路线仍共用同一个 `Vibration.toml`；预设列表也是共用一份（靠 UI 过滤器分流）。
- **新方案**:
  * 文件隔离: S1 (ACT) 路线读写 **`Vibration-ACT.toml`**；S4+ 路线读写 `Vibration.toml`。
    换算集中在 `AppConfig::vibration_route_path_for`，`save_vibration_to_file` /
    `load_vibration_from_file` 内部按 `vib_legacy_client` 选路 —— 全部调用点无需改动。
  * 路线切换 (`switch_vibration_edition_to`): ① 当前这套落进旧路线的文件；② 换路线标记；
    ③ 载入新路线的文件（存在即整份覆盖）。首次使用该路线 → 置出厂默认，由 GUI 套用该路线的
    内置默认预设 (S1 →「ACT1 特供」, S4 →「默认」)。两个快照槽随之下线。
  * 老用户升级迁移 (`migrate_legacy_vibration_file`): 当前在 S1 路线且 `Vibration-ACT.toml`
    尚不存在时，把现有 `Vibration.toml` **整体 rename** 过去（原文件消失 → S4 首次切换按出厂
    默认重建，ACT 特调不会漏进 S4）。已有 ACT 文件时不动。
- **设置页顺序保护**: 客户端版本下拉在设置对话框里会先改 `vib_legacy_client`, 而 `vibration`
  仍是旧路线那套 → 原来紧跟的 `save_to_file` 会把旧参数写进**新路线**的文件, 覆盖新路线原有的数据。
  新增 `AppConfig::save_config_only` (只写 Config.toml), 该接入点改用它; 震动文件交给
  `switch_vibration_edition_to` 自己按路线读写。
- **健壮性**: 迁移 rename 失败退化为 copy（文件被占用时也不静默丢数据）；
  `force` 载入按"整份覆盖"处理预设列表（不再残留另一条路线的预设）；
  设置页切路线改到第一次 `reload_config` 之前（不留"新路线标记 + 旧路线参数"的错配帧）。
- **导出预设按路线分流**: S1 路线导出文件名带 `-ACT`（通用设定 `vibration_export-ACT_<时间戳>.toml`；
  职业导出因 ACT 名册职业名自带 `-ACT` 自然带标记），导出内容里注明路线；
  导入下拉只列**当前路线**的导出文件 (`job_presets::list_export_files_for_route`)，
  防止把 ACT 档位参数导进 S4（反之亦然）。
- 测试: 路线路径换算 / 两路线文件隔离（含预设列表）/ save·load 自动选路 / 升级迁移（含
  "已有 ACT 文件不覆盖"）/ 导出名与路线过滤 / `save_config_only` 不碰震动文件 /
  force 载入整份覆盖（含预设）；**699 通过 / 0 失败**。
- 实测: 用用户真实配置（S1 路线, 16 个预设）启动 → `Vibration.toml` 迁移为 `Vibration-ACT.toml`,
  参数与 16 预设完整保留，主窗口正常渲染；用户交付版首次启动 (02:43) 已实际完成迁移。
- exe md5 08ff1d13a6539a904094b7a6cf4bc7cc

v24.13 (2026-09-12)
===================
S1 / S4 各存一套震动参数（用户选方案 B —— 切版本不再互相污染）:

- **问题**: 切换客户端路线只改引擎路由 (`engine.legacy`) + 预设可见性，**参数只有一份** →
  S1 下套用的 ACT 特调（「ACT1 特供」/「*-ACT」，含事件强度/衰减/档位）继续作用于 S4，
  实机表现为"切到 S4 手感不一样"；且预设下拉的 `vib_preset_idx` 是会话字段（不持久化），
  显示的"当前预设"未必是实际生效的那套（误导）。
- **方案 B 实现**:
  * 配置层: `AppConfig` 新增 `vibration_saved_act` / `vibration_saved_new` 两个快照槽
    （随 Vibration.toml 持久化, Config.toml 不写）；`switch_vibration_edition_to(from, to)` 负责互换。
  * 运行态: 新增 `AppState::apply_vibration_config(cfg)` —— 把**整套** VibrationConfig
    （59 个字段 + 60 槽参数数组）灌进原子量；函数体由脚本从 `AppState::new` 的字面量机械生成，
    并把 `new()` 的 init_params 改为复用同一 helper（`vibration_params_from`），杜绝两处漂移。
  * GUI: 新增 `SorahkGui::switch_vibration_edition(from, to)`；某路线**首次使用**（无历史快照）时
    自动套该路线内置默认预设（S1 →「ACT1 特供」, S4 →「默认」）；落盘两份配置并热重载。
  * 接入点: 首次「客户端版本」询问（guide）+ 设置页客户端版本应用（settings_dialog）。
- 测试: 配置层互换往返 + Vibration.toml 持久化往返 + 运行态写入校验；**688 通过 / 0 失败**。
- exe md5 2064105f291194d5f9e7101d275b1a4f
- ⚠ 已被 v24.14 取代: 两个快照槽换成"S1 读 `Vibration-ACT.toml`、S4 读 `Vibration.toml`"的
  独立文件方案（预设列表也随文件隔离）。

v24.12 (2026-09-12)
===================
修复「累计事件」把移动流也计入（S4 模式下总事件暴增）:

- **根因**: 环形缓冲消费处的计数口径不一致 —— S1(legacy) 分支是
  `last_injected && etype != VEV_MOVE`，而 **S4(非 legacy) 分支直接 `true`**
  （原注释"新方案维持全量计数"）。`VEV_MOVE` 是 6ms 续期的持续流（走路 10 分钟 ≈ 6 万条）→
  UI「● 已连接游戏 (累计 N 事件)」在 S4 下疯狂上涨。
- **修复**: 抽出 `counts_toward_event_total(etype, legacy, last_injected)` 统一口径 ——
  **两种模式都不计 `VEV_MOVE`**；S1 仍保留"只计真实注入"的老语义；
  移动的细分计数 `rank_type_events[11]` 照常（保持不变）。
- 新增回归测试 `vibration::event_count_tests`。
- 682 测试通过, 0 失败; exe md5 c718d6704d213e7a8953bfeb74de1e03

v24.11 (2026-09-12)
===================
修复宿主「移动通道」被 0 哨兵时间戳挡死（用户适配其他版本时发现的宿主 bug）:

- **根因**: 输出三选一分支的第一道门 `if before(now_ms(), engine.rank_full_until)` 缺 0 哨兵守卫。
  `rank_full_until` 初始/复位为 0（= 无评分族事件）；而 `before()` 是
  `deadline.wrapping_sub(now) as i32 > 0`，当 `now_ms()`（UNIX 毫秒截断 u32）落在"负半区"
  （bit31=1）时 **`before(now, 0)` 恒真** → 每帧都走评分分支 → **移动分支成为死代码**。
  例：`before(2517474289, 0) = (0 - 2517474289) mod 2^32 = 1777493007 → i32 为正 → true`。
  该陷阱只在 now 落进负半区的约 24.8 天窗口内暴露，故长期未发现；且 legacy(S1) 评分分支把
  战斗输出 `max` 了进来 → 表现为"战斗/评分照震，**只有移动不震**"（城镇走路完全不震）。
- **修复**:
  * 新增 `rank_window_active(now, until) = until != 0 && before(now, until)`，评分门改用它；
  * **顺带修活移动分支内的同类 0 哨兵**：`move_pace_until` 初次为 0 时裸 `!before(now,0)` 恒 false
    → 步频计时器永不武装（且 `0 - now` 下溢/相位错乱）——该分支此前是死代码所以一直没暴露。
    新增 `expired_or_unset()`（0 = 需武装）并用 `wrapping_sub`。
- 新增 3 条回归测试：`vibration::time_sentinel_tests`（裸 `before(now,0)` 陷阱 / rank 门判 0 非激活 /
  节奏计时器首用即武装）。
- 680 测试通过, 0 失败; exe md5 707e5a7fa3bdebc3108a6d49a263fee4

v24.10 (2026-09-12)
===================
「仅用连发」模式收口 + 关于页文案:

- **仅用连发 (dfo_player=false) 隐藏震动入口**:
  * 经典模式: 隐藏新增的震动快捷条; 右上「震动参数实时生效」文案也一并隐藏
  * 连发页状态卡 (hero): 不再显示「震动: 开/关」开关
- **选「仅用连发」时震动默认关闭** (用户要求: 入口藏了就不该后台还在驱动):
  * 首次询问选「不是, 仅用连发」→ 立即 `vibration_enabled=false` + Vibration.toml `enabled=false` 落盘
  * 设置页取消勾选「DFO 震动功能」→ 同样关闭并落盘 (两处口径一致)
  * 启动归一化: `dfo_player=false` 但 Vibration.toml `enabled=true` (历史数据) → 自动关闭并落盘;
    `AppState` 运行态改为 `enabled && dfo_player`
- **关于页文案**: 标题「🌸 Sorahk 🌸」→「**DfoVibration-Sorahk**」;
  「🌸 灵感来源: 春日野穹」→「**DOF特化版本，同时也泛用**」(其他语言同步改为对应译文)
- 674 测试通过, 0 失败; exe md5 b2d65233e176e8cb23562999bf09bb3e

v24.9 (2026-09-12)
==================
手柄映射页「一屏放下」重构（用户要求：不要滚轮下拉）:

- **实测**: 内容高 715px > 可用 599px（窗口 1180×760 逻辑像素，DPI 1.25）；逐块瘦身到 ~590px：
  * 状态卡: 删掉独立「三步引导」行（改挂「一键校对」按钮悬停提示）与「不再提示」按钮（104 → 67）
  * 主卡: 手柄图宽度上限 400（高度按原图比例跟随）；空状态不再单独成卡——两个入口并排一行、去掉 64px 大图标
  * 快速连接: 「连发 / 震动」并排一行；行距与控件高压缩（interact 26→22, item_spacing 8→4）；
    ACT1 注入说明改悬停提示
  * 映射总览: chips 用 11px（新增 `Theme::badge_clickable_sized`），两行**平衡切分**
    （贪心填充会把最后一条挤到第三行 → 出现「+1」）；并硬性上限两行（超出并成「+N」）→ 卡片高度恒定
  * 去掉页面尾部 12px 空距
- **结果**: 1475×950 窗口下整页（状态卡 + 手柄图 + 快速连接 + 映射总览 20 条）**一屏显示、无需滚动**
- 674 测试通过, 0 失败; exe md5 6592b8643c75bc111219b1f3b09a747e

v24.8 (2026-09-12)
==================
手柄映射页排版重构（用户反馈：页面过长、文字会超出）:

- **修掉横向溢出（根因）**: 主卡片两栏原为「左列固定宽 + 右列 `set_min_width(available_width())`」，
  在水平布局里会撑破窗口 → 整页出现横向滚动条 → 底部 chips 的 `horizontal_wrapped` 失去换行宽度被截断。
  改为**显式宽度两栏**（左列 `clamp(总宽×0.5, 320, 620)`，右列取剩余宽度）→ 无横向滚动，长文本正常换行。
- **去重复引导**: 删掉独立的「怎么用: 一步一步来」卡片，三步说明并入顶部状态条一行（右端保留「不再提示」）；
  空状态卡里重复的 ①②③ 三段说明删除，只留一句结论 → 页面显著变短。
- **收敛纵向**: 手柄图宽度上限 800→620（高度按原图比例，主卡不再过高）；空状态与「快速连接」按钮高度 36→32、
  留白收紧。
- **标签不溢出（真正修好换行）**: 底部「我设好的映射」chips 原先用 `horizontal_wrapped`，但该卡所在的
  父级宽度不受限时它**根本不会换行**（实机表现为 chips 一路向右溢出被窗口裁掉）。改为**按 `clip_rect`
  可见宽度手动分行**（测量每个 chip 文本宽 → 放不下就落到下一行）；同时单条 chip 超过 22 字截断 + 悬停看全名。
- 674 测试通过, 0 失败; exe md5 f4729fce907ae6416599379ed082325f

v24.7 (2026-09-12)
==================
清理与潜在 bug 排查（用户要求：编辑次数太多，排查潜在 bug / 无用代码）:

- **真 bug 修复 — `classic_mode` 被覆盖**: `SorahkGui` 构造里先算
  `let classic_mode = config.classic_mode && !config.minimal_mode;`（双标记同真时极简赢），
  紧接着又被 `let classic_mode = config.classic_mode;` 覆盖 → **守卫失效**，
  配置里 `classic_mode`/`minimal_mode` 双真时会两个外壳打架。已删除覆盖行。
- **潜在劫持修复**: 手柄页"识别态按下手柄键自动选中"增加**窗口聚焦**条件；
  离开页面时清掉 live XInput 边沿缓存 (`gp_prev_xinput`)。否则工具开着去玩游戏时按手柄键
  会误进"设键盘键"捕获态, 把游戏里的按键悄悄记成映射。
- **死代码清理**: 删除已无调用的 `PadCaptureCandidates::preferred` 与 `find_slot_by_trigger`
  (及其测试); 删掉 turbo 页 3 个未使用 import; 去掉几处 no-op `mut` / 冗余赋值 / 重复文档注释。
- **有意保留 (非死代码但当前无引用)**: `hid_layout.rs` 的"标准布局/报告描述符"系列函数、
  `GamepadSlot.default_trigger` 字段 —— 属已完成未接线的遗留设计 (HANDOFF 记为后续摇杆奔跑/
  标准布局用), 删除会丢计划, 故保留并在文档标注。
- clippy 0 error; 674 测试通过, 0 失败; exe md5 d19147cb37a895033d257f43138bf340

v24.6 (2026-09-12)
==================
修复：进游戏后「简易奔跑 / 重推奔跑」失效（v24.2→v24.5 引入的回归）:

- **根因**: 给「手柄按键快速映射」加监听态时启用了**捕获模式**。捕获模式会让 XInput 走
  `handle_capture_mode_xinput`, **跳过 `handle_normal_mode_xinput`** —— 而奔跑重推阈值
  (`run_thr`) 正是在那里注册的, 摇杆方向按键也在那里派发。结果: 进游戏后方向不派发、
  阈值恒 0 → 简易奔跑 (双击) 与重推奔跑全部失效。
- **修复**: 监听态改用**实时状态匹配**, 不再启用捕获模式:
  - XInput 侧新增 `AppState::publish_live_xinput(vid, 位图)`
    (识别中由 `xinput.rs::handle_normal_mode_xinput` 发布, 捕获模式开启时清空)
  - 手柄页新增 `trigger_matches_xinput` + `gp_newly_pressed_slot_xinput`,
    与原有 raw 实时匹配并列; 原始命名与 XInput 命名两种槽位都能"按手柄键即选中"
  - 只在手柄页 + 识别态 + **窗口聚焦**时自动选中, 游戏中不会抢输入
- 回归测试: `test_run_mappings_lookup_for_stick_directions` (映射表/触发键解析/run 标志)
  + `xinput_live_match`; 676 测试通过, 0 失败
- exe md5 644eb4d08cb2cbb065dfcffa103d12d8

v24.5 (2026-09-12)
==================
经典模式 + 快速映射 UI 收尾:

- **经典模式 · 震动快捷条**: 在「预设管理」上方新增一条 (用户要求) ——
  震动 开/关 + 震动预设切换/应用 + 「震动调校 →」跳转「通用型震动设定」;
  预设与应用逻辑复用震动页 (`apply_general_vibration_preset`), 两处永远一致
- **经典模式 · 移除「手柄映射」页签** (用户要求: 经典就应该经典);
  手柄映射仍在完整模式侧边栏可用
- **快速映射完成页** (新增 `GpFlow::MapDone`): 走完「选键 → 设键盘键 → 确认」后显示
  「本次按键已完成修改。您可以继续按下其他手柄按键，为其编辑新的映射键位；也可以留在当前页面，
  继续调整此按键的设置。」+【编辑本映射】按钮 (回到本键第 1 步)。
  完成页**不独占输入** —— 直接按其他手柄键即可继续映射 (监听态自动重开)
- 673 测试通过, 0 失败; exe md5 9e274ae6b09de0bae7e64c2de33f7488

v24.4 (2026-09-12)
==================
UI 修复: 新增映射占位符 + 重新校对串位自愈:

- **新增映射**: 触发键初始为空, UI 显示「尚未捕获触发键」(旧默认 `"A"` 会让人以为已经捕获了 A 键);
  未捕获时「保存修改」置灰 (悬停提示先捕获), 列表行也不再画空键帽
- **重新校对串位自愈**: 触发键若已被别的槽位占用 (旧版本 bug 留下的错位数据), 不再拦下报
  "请换一个键按", 而是**把该键移到当前槽位并清掉旧槽位** —— 实机症状: 旧配置把 A 键的位组合
  记在了 X 槽, 重新校对按 A 被拦下, 看起来就是"A 键被识别成 X 键"
- 新增回归测试 `set_slot_trigger_moves_conflicting_key`
- 673 测试通过, 0 失败; exe md5 23f03cfa9a8ba50c12876dfd937dea89

v24.3 (2026-09-12)
==================
手柄校准「串键」根治 + 快速映射识别已校对键 + 窗口调回 300ms:

- **窗口 600ms → 300ms** (用户实测: 600 反应迟纯; 200 兜不住回弹)
- **串键根因**: 一次手势 (尤其摇杆按下 L3/R3 伴随轻微推动) 会在同一次按压里产生多个输入。
  实测配置: `手柄右摇杆·按下` = `GAMEPAD_045E_RS_Click+GAMEPAD_045E_RS_RS_Right`,
  `手柄左摇杆·左` = `GAMEPAD_045E_LS_Down` (斜推被记成单方向) —— 光靠时间窗口分不开, 必须看内容
- **稳方案**: 按压窗口按通道各收候选 (`PadCaptureCandidates`), 由 `pick_calibration_device` 按槽位类型挑选:
  - 摇杆槽位 (方向 / 摇杆按下) **优先 XInput 通道** (摇杆"方向 + 回弹"只有 XInput 有确定语义),
    并按槽位过滤 id: 摇杆按下只留 click, 方向只留对应摇杆的四个方向;
    过滤后**不是恰好一个 → 提示重按, 不推进步骤** (斜推 / 缺 click 都不会记脏)
  - 非摇杆槽位原始命名优先; 只有 XInput 时要求恰好一个键, 多键 → 提示重按
- **可见性**: 校准向导里每个已完成热点直接显示"记下的触发键简称"
  (`✓右摇杆按下=045E_RS_Click`), 串键一眼可见, 不用再跑去连发映射区逐条核对;
  提示语按槽位类型区分 (摇杆按下 / 推方向 / 普通键)
- **快速映射**: 「手柄按键快速映射」改走捕获通道匹配已校对的触发键
  (`find_slot_by_trigger`, 原始命名与 XInput 命名都认) → 按手柄键**直接选中槽位并开始设键盘键**
  (旧实现只比对 live 原始位组合, 认不出 XInput 命名的槽位)
- 已知遗留: 真组合键名字经 `util::key_combo_parts` 会被多加一段设备前缀
  (`RS_Click+RS_Right` → `..._RS_RS_Right`), 连发映射页仍可能生成; 本次未动 (校准已拒绝多键输入)
- 新增回归测试 10 条 (`pad_capture_reconcile_tests`): 窗口边界/双通道/回弹过滤/斜推拒绝/命名匹配/简称
- 671 测试通过, 0 失败; exe md5 cd067b8eae75431e0cf12939fc7a9ad0

v24.2 (2026-09-12)
==================
修复「一键校对手柄按键」一次按压连吞两个槽位:

- **真凶**: 同一次物理按压被**两条通道各上报一次** —— 原始 HID 通道 (`GenericDevice`, 命名
  `GAMEPAD_<vid>_<pid>_<tag>_H<usage>`) 与 XInput 通道 (`XInputCombo`, 命名 `GAMEPAD_045E_<按钮>`)。
  校准向导「一条事件 = 一步」→ 依次按下时一次按压连推两步: A 槽拿原始命名、B 槽拿 `GAMEPAD_045E_A`
  (用户 `Config.toml` 里两条 system 备注逐字复现)
- **修复**: 新增 `PadCaptureReconciler` —— **按压窗口 (600ms)** 内**所有**事件 (原始 HID + XInput 两条通道,
  以及摇杆按下 L3/R3 的物理回弹) 都算同一次按压, 只产出一个代表, **原始命名优先**;
  窗口到期才产出 (每帧 `tick` 结算, 有事件时也可即时结算)。校准向导与单槽「校键」(`AwaitPad`)
  统一走 `poll_pad_capture()`
- **窗口 200ms→600ms + 语义收紧 (用户实测定值)**: 右摇杆按下后回弹 (`RS_Click` → `RS_Right`)
  迟到几百毫秒才上报, 旧逻辑只合并"不同命名空间", 回弹会覆盖下一个槽位
  (实机 `Config.toml`: `手柄右摇杆·按下` ← `045E_RS_Click`, 紧接着 `手柄十字键·上` ← `045E_RS_Right`)
- 附带说明: 连发映射区「逐条捕获」此前看着正常, 是因为收到第一条就关通道 (抢先), 非合并
- 新增回归测试 `pad_capture_reconcile_tests` 6 条 (双通道同帧/反序/单通道/摇杆回弹吞并/超窗两次按压/600ms 边界)
- 663 测试通过, 0 失败; exe md5 3c2c54b511b994699a513c570a567c02

v24.1 (2026-09-12)
==================
修复"按一次出两个键"(待实机验证) + 交接文档:

- **定位**: 校对新建的键位映射默认"连发开启"(`turbo_enabled=true`), 配合全局 5ms 间隔,
  按一下会立刻补发一次 → 游戏端表现为"按一个键出两个"
- **修改**: 新建手柄映射**默认不再连发** (需要连发的键可在「更多设置」单独勾选)
- 新增交接专文 `docs/交接_手柄按一次出两个.md` (症状/已确证事实/假设排序/**插桩取证步骤**/红线)
- 651 测试全绿; exe md5 a66ff53bbae3a964ec3275d46c82b25a

v24.0 (2026-09-12)
==================
手柄页重构为"连发映射的可视化 UI":

- **删除全部独立机制** (校准表 / 标准 usage 表 / 自造 XInput 命名) —— 这些正是"按一个键触发两个"的根源
- 校对某键 = 写一条**普通映射** (与连发映射页完全同构), 并自动记录**系统备注**
  `手柄<键名>（系统）`; 手柄页只按该备注关联映射, 读写它的目标键与开关
- 高亮/自动选中 = 拿该键映射的触发键与当前原始位组合比对 (连发同款编码)
- 未校对的键不再"猜"名字, 主按钮置灰并提示先校对
- 旧备注 `手柄·X` 进页面时自动迁移为 `手柄X（系统）` (触发键/目标键保留)
- 备注随映射保存在预设里 → 有线/USB/蓝牙各用各的预设, 互不冲突
- 651 测试全绿; exe md5 7cced21132953401ab2431287f8d20bb

v23.1 (2026-09-12)
==================
修复启动失败 + 清理历史映射:

- **修复**: 配置里存在无法解析的触发键时, 程序**完全起不来** (报 Failed to initialize);
  现在这类坏条目只会被跳过并记录, 不再阻断启动
- **清理**: 进手柄页/校准完成时自动清理历史遗留映射 —— 删除坏条目、旧 XInput 命名条目
  (存在原始校准设备时)、同键位重复条目; **预设里的旧映射一并清理**
- 663 测试全绿; exe md5 31d9a3640df6ff574017d483320b720b

v23.0 (2026-09-12)
==================
手柄页彻底改走"连发同款"原始通道 (一劳永逸):

- **删掉全部"标准化手柄"假设** (标准 usage 表 A=1/B=2/…、hat/扳机轴判定):
  它们对非标准手柄必错, 并与 XInput 命名映射并存 → **按一个键触发两个**
- 校准/高亮/自动选中/映射触发名 **一律使用连发同款原始报文位组合**, 按键与方向不再区别对待
- **校准完成后自动清理**该手柄的重复/过时映射 (每个键位只留一条, 优先原始命名)
- 不再自动生成 `GAMEPAD_045E_*` (XInput) 命名映射 (纯官方 Xbox 手柄仍可用)
- 657 测试全绿; exe md5 38576cf2ac8edd1cf98930d6afef42f5

v22.8 (2026-09-12)
==================
修复校对卡在十字键:

- **修复**: 一键校对走到十字键/摇杆方向会卡住 (旧版方向步只认标准 hat/摇杆方向位,
  非标准手柄认不出 → 永久停在该步)
- 现在方向步**语义认不出时自动改用连发同款原始识别学习该方向** —— 既能通过本步,
  之后也能正确点亮与映射 (不会再"假装成功")
- 665 测试全绿; exe md5 581105b056c8eb1e3c7def88f4bfdfcf

v22.7 (2026-09-11)
==================
校准隔离 + 完成项变色:

- **修复**: 校对期间按十字键/摇杆不再弹出"设置键盘映射"面板 (旧版会被自动选中抢走校准状态)
- **校对期间不派发任何手柄映射/功能** (不注入游戏、不触发切换键/预设切换), 只保留实时识别
- 校准进度行**分色**: 已完成绿色 ✓ / 当前强调色 ▶ / 未完成灰色
- 已校对设备状态条显示绿色「已校对 · 识别中: …」
- 665 测试全绿; exe md5 87eef13fae80315036ae9bb2404ee486

v22.6 (2026-09-11)
==================
改名 + 醒目色 + 向导覆盖全部热点:

- 「一键校准按键」→ **「一键校对手柄按键」**, 按钮改为实心强调色 (更显眼)
- 「开始识别手柄」→ **「手柄按键快速映射」**
- 一键校对向导从 12 个实体键**扩展为全部 24 个热点圈** (面键/肩键/扳机/系统键/摇杆按下 +
  十字键四方向 + 左右摇杆四方向), 一次性校完, 不漏; 方向类只需推到位/按住即可
- 空状态说明替换为三步指引 (先校对 → 再映射 → 有遗漏再改)
- 665 测试全绿; exe md5 6fd7754f8039b4c533c7e251491f9f91

v22.5 (2026-09-11)
==================
一键校准改为"必须走完全部按键":

- 取消「跳过这个键」—— 必须把 12 个键全部按一遍向导才结束, 保证不漏键
- 新增完成进度行 (✓已按 / ▶当前 / 未按), 一眼看出还差哪些
- 完成提示带上数量: "一键校准完成 — 12 个键都记住了"
- 663 测试全绿; exe md5 5d338d50dc57be791516cdbd7c093c93

v22.4 (2026-09-11)
==================
新增「一键校准按键」向导:

- 点顶部「一键校准按键」后, 按提示把手柄上的键**逐个按一遍再松开** (约 10 秒),
  自动记住整套键位 —— 不用再逐槽位手动校准
- 顺序: A → B → X → Y → LB → RB → LT → RT → Back → Start → L3 → R3
- 可「跳过这个键」「取消校准」(已按过的键保留); 重复按到已记录的键会提示并停在本步
- 663 测试全绿; exe md5 1fab0b084d886c56c343cd2bb30a4044

v22.3 (2026-09-11)
==================
校键改用连发同款识别通道 (用户实测 LT 无反应):

- 「校对手柄键位」不再依赖语义 usage 识别, 改为**与连发映射完全相同的原始报文识别** ——
  按一下手柄上的键 (或扣一下扳机) **再松开** 即可抓到, LT/RT 等都能识别
- SVG 高亮同步改读这条通道; 校准过的设备不再套用"微软标准键序"默认表 (避免 Back 按下时 LB 一起亮)
- 657 测试全绿; exe md5 43cb271ac7fabe18afef8c7e665e21da

v22.2 (2026-09-11)
==================
潜在 bug 审计修复 (用户要求复查):

- **扳机阈值假阳性**: 居中/双极轴的手柄, LT/RT 会静止就判"按下"; 改为按"与空闲基线的偏移"判定
- **流程锁补漏**: 右侧「我设好的映射」标签在捕获/确认中不再能点击跳走
- **陈旧绑定泄漏**: 取消/停止识别后不再残留上一次的校准绑定
- **空映射污染**: 只按了手柄键但没按键盘键就取消, 不再留下触发键错位的空映射
  (映射改为确认时才按正确触发键创建)
- 653 测试全绿; exe md5 9f7995dd3d8d516b591244f30dee4d99

v22.1 (2026-09-11)
==================
手柄页第二轮修正 (用户反馈):

- **流程锁**: 等键盘键期间按其他手柄键不再改选槽位 (旧版会把 A 键的键盘键错记到 B 槽位);
  必须按键盘键或点「取消」走完流程
- **鼠标污染修复**: 手柄页捕获键盘键时排除鼠标按键 —— 旧版点 SVG 热区会被记成"鼠标按键"
- **手柄识别错位根治 (按设备校准)**: 你这台手柄的 HID 按键顺序不是微软标准序
  (Back 报 usage 5、Start 报 usage 6), LT/RT 是模拟轴而非按钮 → 旧版 Back 亮成 LB、Start 亮成 RB、
  LT/RT 完全不亮。现在「校对手柄键位」会记录**你手柄上的真实按键**, 按设备存进 Config.toml,
  之后高亮/映射都按它来 (每台手柄校准一次即可)
- **LT/RT 扳机支持**: 模拟轴 (Z/Rz) 超半量程即算按下, 现在能点亮也能映射
- 651 测试全绿; exe md5 83de55326c52950fd0dd86837e52fdcf

v22.0 (2026-09-11)
==================
手柄映射页交互重构 (用户选定"方案 B", 要求"一步一步来"):

- **交互逻辑重构为状态机** (`gui::types::GpFlow`): 每一步只呈现一个主操作;
  「取消」永远只回退一层 (回到已选中), 不再顺手退出识别; 新增显式 **「停止识别」** 入口
- **修复**: 识别中途按错一个手柄键会把识别关掉 (之后按键不再亮起)
- **修复**: 未识别手柄就点图上的键, 会写死一条永不命中的映射却显示"设置成功";
  现在无识别手柄且无 XInput 时会**拒绝建立映射**并就近提示"请先识别手柄"
- **修复**: 切到其它页面后手柄页捕获仍会吃键盘/手柄输入
- **「更多设置 (连发 / 奔跑)」改为就地展开**, 不再跳转到连发页
- 顶部新增**识别状态条** ("识别中: <设备名> · [停止识别]"); 成功提示不再挂"退出识别"按钮
- 删除死代码 (`QuickGamepadPending` 待确认条 / 旧散落状态字段 / 死状态 `raw_hid_status`)
- 640+ 测试全绿; exe md5 f356ab77820d30b5b682b49b3cde76b1

v21.9 (2026-09-11)
==================
交互调整 (用户要求):

- 捕获后先显示「已捕获: X」, 点 **【确认】** 才写入配置 (新增确认步骤, 防误绑定)
- **不再用 Esc 取消捕获** —— Esc 现在和其它键一样可以被捕获并记录为映射; 取消改用面板按钮
- **不再从鼠标捕获** (避免映射污染); 鼠标映射改由新的 **「鼠标映射…」小弹窗**显式选择:
  【滚动】(上/下/左/右滚) 或 【点击】(鼠标左/右/中键)
- 点【取消】会**同时退出识别并回到初始界面**
- **删除「识别我的手柄」卡片** (空状态的手柄图标按钮已替代该功能), 引导文案同步更新
- 轻提示的【知道了】改为 **【退出识别】**, 点击即停止识别并回到初始界面

v21.8.1 (2026-09-11)
==================
简化 (用户反馈"语义和功能过于复杂"):

- 删除槽位面板里的「改成别的键」按钮 (少一个入口)
- 「校对手柄键位」只取**一个**手柄键, 按下即完成 —— 取消组合键累计
  (组合键只用于键盘映射; 校对就是确认"这个槽位是哪个键")
- 主按钮「改键盘按键」文案改为 **「本键位键盘映射修改」**
- 空状态改造成**「开始识别手柄」入口**: 手柄图标变为可点击 (强调色 + 悬停放大 + 手型光标),
  下方新增描边按钮「开始识别手柄」, 说明文字改为三步引导 (先识别 → 按手柄键 → 按键盘键)

v21.8 (2026-09-11)
==================
体验重构 (以"纯小白 + 手柄玩家"视角):

- **两步完成一个映射**: 点手柄图上的键 (或识别态下按手柄键), 若该键还没设键盘键, 会自动进入
  "按键盘"捕获并留在本页; 按下键盘键**立即生效**并弹出轻提示 "已设置: 十字键·上 → Q"。
  (原流程要跳去连发页编辑面板, 体验割裂)
- 兜底: 正等着按键盘时又按了手柄键 → 自动改选刚按的那个手柄键 (不会"按了没反应")
- **文案去术语**: 「触发键」→「手柄按键」, 「目标键」→「键盘按键」;
  连发/奔跑收进「更多设置 (连发 / 奔跑)…」(渐进披露);
  槽位面板顶部用大字给出一句话结论 **「十字键·上 键 → 键盘 Q」**
- 手柄页顶部新增可关闭的**新手引导条** (两步怎么用 + 第三方手柄先识别)
- 识别卡改名「识别我的手柄」(第三方手柄先点这里; 官方 Xbox 手柄可跳过), 并上移到映射一览之前
- 总览卡改名**「我设好的映射」**, 每条写成 **「十字键·上 → Q」**(原来只有键名+数字)
- **删除映射改为二次确认** (第一次点变红字「⚠ 再点一次确认删除」, 可取消) —— 原为一键即删不可恢复
- 空状态文案全部改为给出下一步动作

v21.7d (2026-09-11)
==================
缺陷修复:

- 手柄映射页「已配置的按键」卡片被撑得极高 (用户截图反馈): 根因 = 徽章文字在
  `horizontal_wrapped` 行尾剩余宽度不足时**逐字换行** (「右摇杆·左」竖成一列) →
  卡片高度暴涨。修复: 徽章一律不换行 (`TextWrapMode::Extend`), 全局生效
- 组合键重复按键 / 顺序不规范 (用户反馈): 捕获触发键或目标键时, 重复按键不再重复出现,
  且严格按 GUI 顺序排列 (修饰键 → 上/下/左/右 → 手柄按键 → 手柄轴方向 → XInput 按键 → 其余)。
  例: 上+上+空格 → 上+空格; 下+上+空格 → 上+下+空格

体验优化 (手柄映射页):

- 槽位面板精简: 删除「② 捕获目标按键 (键盘/鼠标)」; 「① 捕获手柄触发键」改名为
  **「校对手柄键位」**; 新增「编辑映射…」一键打开该槽位的映射编辑面板 (目标键/连发/奔跑)
- 「校对手柄键位」改为**累计式捕获**: 可连续按多个手柄键拼成组合键, 并显示已捕获的键位标签
  (点 × 移除); 捕获期间**隐去面板其余内容**, 只留"已捕获 + 完成/取消", 避免干扰判断
- 识别态 (第三方手柄实时识别) 下按下手柄键 → 自动选中该槽位; 若该槽位还没设目标键,
  **直接打开它的映射编辑面板** (按下即进入设置), 已设过的不再跳转
- 「开始识别」不再批量生成空映射 (会把映射列表堆满空行); 改为按下按键/打开编辑时**按需创建**,
  且第三方手柄的触发键自动使用语义名 (保证真正生效)
- 「已配置的按键」只列已设目标键的槽位 (真正生效的), 其余用一行提示带过

v21.7c (2026-09-11)
==================
缺陷修复:

- 预设管理「切换键」输入框无法输入任何键位 (用户实测反馈):
  根因 = 共用文本输入框 text_input 用 `TextEdit::id_salt(id)` 注册控件,
  egui 实际 id 为 `ui.make_persistent_id(id)` (随 Ui 层级变化), 与调用方传入的
  `Id::new(id)` 不相等 → 调用方 `has_focus(id)` 恒为 false → 该输入框每帧把用户
  输入重置回原值。改用绝对 `.id(id)` 兑现文档契约 (新增离屏回归测试锁定)
- 切换键与映射冲突根治: 保存时校验不得与**任何预设的任何映射触发键**重名,
  也不得作为组合键的任一组件 (如映射 CTRL+F6 与切换键 F6 亦冲突);
  连发/组合性质的映射给出明确冲突说明

功能增强:

- 预设「切换键」改为**键位构建器** (用户反馈: 能输汉字 / 捕获只能一个键):
  * **只捕获不手输** —— 移除文本输入框, 从根上禁止汉字等非法内容
  * 捕获结果以**键位标签**显示 (可点 × 单独删除), 可**连续捕获追加**组成组合键
    (如先捕获 CTRL 再捕获 F6 → CTRL+F6); 同名键自动去重
  * 新增「⌫ 删除末键」; 修饰键 (CTRL/SHIFT/ALT) 保存时自动前置
  * 保存校验新增: 组合键不能只有修饰键 (CTRL+SHIFT 这类永远触发不了, 直接拦下);
    手柄的多个按键暂不支持合成一个切换键 (会给出明确提示)
- 预设「切换键」新增「🎮 捕获」按钮: 点一下后按键盘任意键或手柄任意键即自动填入
  (此功能主要为手柄准备)
- 预设切换键支持手柄按键: 手柄类切换键由 XInput/RawInput 输入线程全速检测
  (配置变化时推送绑定表), 命中后经队列回到 UI 线程执行切换;
  键盘切换键仍走原 GetAsyncKeyState 轮询
- 手柄映射页新增「第三方手柄 · 实时识别」(替代原「一键标准布局」, 用户反馈原方案鸡肋):
  * 点「开始识别 (读取该手柄)」→ 显示识别到的标准键位清单, 并**自动把空槽位绑定**到该手柄
    (触发键 = 语义名, 目标键留空; 已经绑过别的手柄且设了目标键的槽位不覆盖)
  * **按实体手柄按键, 左侧 SVG 手柄对应键位实时亮起** (白环+高亮填充):
    面键/肩键/扳机/Back/Start/L3/R3 走 HidP_GetUsages; 十字键走 hat 值;
    左右摇杆方向走"与空闲基线的偏移"判定 (30% 死区)
  * 方向键 (十字键/摇杆) 也会派发语义按键事件, 因此**可直接设目标键并生效**
    (不含三区奔跑; 奔跑仍为后续计划)
  * 「停止识别」可随时关闭实时识别
- 新增「第三方手柄 · 一键标准布局」(手柄映射页): 读取手柄能力 (HidP_*), 按通用
  约定一键生成标准按键映射 (触发端), 目标键留空由用户填写
- 新增 HID 报告描述符纯解析器 (hid_layout, 零新依赖) + 单测: 可离线把描述符字节
  翻译成按钮位偏移/轴量程 (为不遵循通用约定或需 bit 级精度的场景预留)

技术说明:

- 运行时路线经本机实测确定: 目标设备 20BC:5159 (A1 手柄) 不支持
  IOCTL_HID_GET_REPORT_DESCRIPTOR (DeviceIoControl 恒返 ERROR_INVALID_FUNCTION),
  故放弃"读描述符算 bit 偏移", 改用 Windows 自带 HidP_* 语义 API
  (RIDI_PREPARSEDDATA + HidP_GetButtonCaps/GetValueCaps/GetUsages);
  实测该设备解析出 10 按钮 (A/B/X/Y/LB/RB/LT/RT/Back/Start) + 6 轴
- 语义事件与既有 bit-hash / 手动捕获命名空间互不重叠, 两套映射可共存;
  仅在配置存在语义触发键或正在实时识别时才走 HidP 路径 (否则零开销);
  全松开时 HidP 返回 USAGE_NOT_FOUND 已按"空集"处理 (防卡键)
- 619 测试全绿 (基线 557 + 62 新增)

v21.5 (2026-09-11)
==================
功能增强:

- 奔跑功能文案改名: 【1×双击】→【简易奔跑】, 【奔跑】→【重推奔跑】
  (含界面提示、代码注释与中/繁/英/日四语言 hover; 配置字段名不变, 旧配置零迁移)
- 【简易奔跑】与【重推奔跑】互斥: 勾选其一时另一项变灰不可勾选
  (连发页编辑面板 / 手柄快捷捕获确认条 / 设置弹窗三处);
  历史数据两项同时开启时按"重推奔跑优先"自动清理, 避免灰死锁

v21.4 (2026-09-11)
==================
优化:

- 编辑面板「删除该映射」按钮从标题行右端移到底部操作行最右端
  (与 保存修改/取消 同行), 消除"点完列表行『编辑』按钮后同位置再点即误删"的隐患;
  悬停补充"不可恢复"提示

v21.3 (2026-09-11)
==================
功能增强:

- 「重推阈值再检测」改为急推判定 (功能修正): 缓慢推进越过重推线 = 继续走路
  (只是想走深一点, 不触发); 在「二次敲击间隔」窗口内从轻推区猛冲过线
  (推进量 ≥ 重推区间一半) = 瞬间推入 → 触发奔跑双击
- 「二次敲击间隔」由此具备双重作用: 双击序列间隔 + 急推判定窗口

v21.2 (2026-09-11)
==================
优化:

- 奔跑双敲节奏: 松开长按后立即接第一次敲击 (去掉前置空窗, 角色脚步不停),
  「二次敲击间隔」只用在 走→跑 切换点 —— 视觉序列 = 走→走→轻微一顿→跑
- 「二次敲击间隔」悬停说明同步更新 (视觉顿挫调小 / 判定不出双击调大)

v21.1 (2026-09-11)
==================
功能增强:

- 新增「重推阈值再检测」开关 (默认开启): DNF 双击判定不把"一直按住"算敲击,
  走路中推过重推线只补一次松按仍判走路 (用户实测定案); 开启后重推时模拟
  完整双击序列 松开→敲(80ms)→松开→再按住 → 判定双击→奔跑
- 关闭 = 退回 v21.0 单次松按模式 (双击判定宽松的游戏用)

v21.0 (2026-09-11)
==================
功能增强:

- 摇杆三区奔跑 (用户设计方案): 死区/轻推区/重推区 —— 轻推摇杆=方向键按住(走路),
  推过「重推阈值」(默认 80%, 可调 50-95%) = 自动补一次"松开→再按下"触发奔跑
- 新增「奔跑」开关三处入口: 连发页编辑面板 (含重推阈值/二次敲击间隔调节)、
  设置弹窗映射行、手柄快捷捕获确认条
- 勾选奔跑后该映射的 连发/1×双击 引擎侧自动压制 (避免节奏冲突)

修复:

- 「1×双击」在 DNF 奔跑无效的根因: 双击模拟首击只有 5ms (映射时长),
  游戏不计数; 改为 80ms 常量首击 (用户实测: 两次方向敲击需要可辨识的间隔)

技术说明:

- 新逻辑全部 gate 在 run_enabled 后, 不勾选 = 旧行为逐字节不变
- 摇杆幅度检测在 XInput 层完成 (轴值三区状态机 + 迟滞 7/8 + 200ms 冷却),
  新增 InputEvent::RunTap 事件; 单测暴露并修复迟滞计算 i16 溢出 bug

v20.9.1 (2026-09-10)
====================
修复:

- 连发映射键帽配色定稿为「描边式」: 中性键帽底 + 1.3px 彩色描边 + 彩色字
  (紫=手柄 / 橙=鼠标 / 键盘=完全原样式); v20.9 色块填充风格经用户反馈否决后重做,
  编辑面板目标标签底色同步减淡

v20.9 (2026-09-10)
==================
功能增强:

- 连发映射列表与编辑面板键帽按设备类型上色, 一眼分清手柄/鼠标/键盘来源
- 新增按键设备类型识别 utils::key_kind (GAMEPAD 前缀=手柄, MOUSE_*/SCROLL_*/鼠标按键=鼠标)
- 设计系统新增 gamepad/mouse 键帽语义色 (明暗两套)

v20.8 (2026-09-10)
==================
修复:

- 鼠标「移动方向」「滚动方向」选择弹窗塌缩成 ~24px 小圆点的老 bug (modal_window
  构建器点状塌缩, 交接记录第 17 条冻结案结案): 两弹窗改回内联 egui::Window 样板,
  构建器标记禁用 (零调用者)
- 由此修复的下游问题: 方向永远无法选中 → 映射列表恒显示「未设置目标」

文档:

- HANDOFF 新增第 40/41/42 条; 第 42 条 = GitHub 上交流程 (GitHub Desktop + D 盘 clone,
  口令「测试通过，上交到仓库」触发)

0.4.0
=====
Feature enhancements:

- Add multiple target keys support for simultaneous key presses
- Add XInput API integration for Xbox controller support
- Add Raw Input API integration for HID devices
- Add HID device activation system with interactive calibration dialog
- Add mouse movement support with eight-directional control
- Add mouse scroll support with configurable speed
- Add tray icon internationalization support with dynamic language switching
- Add performance optimizations for input processing
  - Multi-tier caching for device information
  - SIMD acceleration for data comparison when available
  - Inline optimization for frequently called functions

UI Improvements:

- Add Device Manager dialog for device configuration and testing
- Add mouse direction and scroll selection dialogs
- Add target type selector in settings dialog
- Add HID device activation dialog with real-time feedback
- Add internationalization support for tray icon menus and notifications

Configuration:

- Change `target_key` to `target_keys` array for multi-target support
- Add capture mode configuration for XInput devices
- Add device baseline persistence for HID devices
- Add per-device API preference settings
- Add movement and scroll speed configuration

0.3.0
=====
Feature enhancements:

- Add multi-language support (English, 简体中文, 繁體中文, 日本語)
- Add language selector in settings dialog with real-time preview
- Add key combination support for triggers and targets (e.g., LALT+A, RCTRL+RSHIFT+S)
- Add mouse button support (Left, Right, Middle, X1, X2)
- Add per-mapping turbo mode toggle with Windows native repeat support
- Add lock-free concurrency with scc containers for improved performance
- Add multi-layer caching for process whitelist, turbo state, and mapping info
- Add combo key reverse index for O(1) lookup optimization
- Add enhanced key capture with comprehensive keyboard support
  - Support for F1-F24, numpad, lock keys, system keys, and OEM punctuation
  - Left/right modifier distinction (LCTRL/RCTRL, LALT/RALT, LSHIFT/RSHIFT)
  - Initial state filtering to prevent false positives
- Add input validation for duplicate trigger keys and process names
- Add comprehensive test suite with TESTING.md guide
- Add `turbo_enabled` configuration field (defaults to true)
- Update timing parameter minimums (input_timeout: 2ms, event_duration: 2ms)

UI Improvements:

- Add turbo toggle button with visual state indication (⚡/○)
- Add localized hover tooltips for turbo toggle in all languages
- Add turbo status display in main window mappings table
- Increase settings window width to 720px for better layout

0.2.0
=====
Feature enhancements:

* Add GUI with anime-style design
* Add interactive settings dialog with real-time configuration editing
* Add configurable light/dark theme support with persistent storage
* Add multi-threaded worker pool with load-balanced event dispatching
* Add process whitelist for application-specific turbo-fire control
* Add Windows Toast notification system with fallback support
* Add About dialog with project information
* Add embedded application icon

UI Improvements:
- Replace tray icon display to use custom icon

0.1.1
=====
Feature enhancements:

* Tray icon support
