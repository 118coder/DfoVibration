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
