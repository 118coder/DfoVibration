# 音频震动 C 模式算法设计 v1

> ★状态更新（2026-11-22，主线填）：本文档按用户指令**暂停定稿于当前版本**，不再续写。
> **C 模式已由实现轮落地完成**：`src/audio_haptics/c_mode.rs`（1653 行）+ 相关接线，
> `cargo test --profile iter --lib` 519 过 / 0 败（新增 16 项合成信号用例），
> 性能实测 1s 音频 C 增量 0.9~2.7ms（预算 <10ms）。实现与设计的 12 条偏差、开启方式、
> 遗留项（实机调参 / web 侧选择器待接线）详见 `docs/交接_C模式实现轮.md`。
> 本文档保留为算法设计依据与历史记录。

> 原始状态：研究 + 设计文档，仅供实施评审；本轮不修改源码、不修改配置、不做 Git 操作。  
> 基准版本：以当前 `SorahkDFO源码/src` 中 v24.60 音频链路为准。  
> 核心结论：C 不是另起炉灶的第三条 DSP 链，而是 **B 模式智能层的下一代**：保留 B 已被实机验证有效的距离/音高/持续时间差异化，再补上多带事件颗粒度、重复节奏抑制、立体声事件路由和明确的事件包络。

## 0. 设计边界与证据口径

### 0.1 本文要解决的问题

现有音频模式的实际关系是：

```text
A：经典基线 = 现有频带电平跟随 + 内容三分类/模式配置 + 攻放平滑
B：A 的基础输出 + smart.rs 的无模型智能增强
C：B 的基础输出 + 多带瞬态/事件状态机/纯 DSP BGM 筛选 + 频带马达路由
```

用户已确认 B 实机效果优于 A。因此本设计的主问题不是“再做一个完全不同的算法”，而是：

1. 保留 B 的近/远、低/高、短/长等已经有效的手感差异。
2. 让一次技能音效、连续技能、BGM 鼓点、语音和环境声在事件层面有更细的颗粒度。
3. 不依赖 GPU、预训练大模型或模型权重文件，仍然保持纯 Rust 实时处理。
4. C 出错时，能在运行中无损退回 B；B 再能无损退回 A。

### 0.2 已实际读取的代码

以下文件均已逐文件读取并以源码为准：

- `src/audio_haptics/mod.rs`：WASAPI 环回、采样格式解码、白名单/暂停/设备状态闸门、引擎线程和输出原子量。
- `src/audio_haptics/dsp.rs`：FFT、频带 RMS、噪声底、瞬态均线、内容分类调用、映射、B 层接入、攻放平滑。
- `src/audio_haptics/mapping.rs`：`EngineParams`、A 的映射曲线、攻放函数、BGM/人声/安静分类规则和预设。
- `src/audio_haptics/resample.rs`：通用低通 + 线性插值重采样器。
- `src/audio_haptics/smart.rs`：B 模式智能层的全部特征、平滑、增益、左右偏置、点击感和时长估算。
- `src/config.rs`：音频参数、B 开关和默认值。
- `src/state/audio_state.rs`：配置到原子量的镜像、B 遥测和输出状态。
- `交接_总集.md` §14.16：历史任务书背景，以及当前 v24.60 已退役 YAMNet/MRX 层的记录。

### 0.3 证据边界

- 当前运行链路以源码为准。历史文档中出现过的 YAMNet、MRX、SuperFlux 和自相关节拍检测，不能直接当成当前实现；当前 `dsp.rs` 的实际路径是频带 RMS + 总能量瞬态均线 + 内容分类 + `SmartEnhancer`。
- 当前 B 的“AI”是界面命名，`smart.rs` 实际是无模型、无网络、无新依赖的规则/DSP 特征融合层。
- 文中“目标”“验收线”“CPU 预算”是实施前的设计约束或待验证指标，不是已经测出的结果；凡未由当前代码证明的数值均明确标注“待验证”。
- WASAPI 官方说明确认：loopback 从 render endpoint 捕获系统混音，只能用于 shared mode；Windows 10 1703 以后支持事件驱动 loopback。当前代码采用轮询模式，且对设备格式做实际解码。参考：[Microsoft Loopback Recording](https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording)。
- BGM 候选方法按经典 MIR/DSP 定义作工程化取舍，本文不声称 HPSS/REPET 等可单独准确识别游戏语义，也不宣称下列公式由论文原样给出。实现前可参考本仓库既有音频调研 [`docs/音频震动模式调研.md`](音频震动模式调研.md) §6 的谱通量/Dixon onset 路线、§7 的 DSP+语义混合边界，以及历史说明 [`docs/HANDOFF.md`](HANDOFF.md) §v24.52 的 SuperFlux/谱平坦度/自相关节拍经验。C 的在线因果化与组合权重属于本设计方案，需用实验验证。

---

## 1. A/B 现状剖析

### 1.1 当前完整链路

```text
默认 Render 设备或 PID 进程环回
  ↓ WASAPI polling，约 10ms tick
Loopback::read()
  ↓ 按 F32/I16/I24Packed/I32 解码，归一化到 -1..1
多声道混音为 mono；同时计算 L/R RMS
  ↓ mono push 到 Analyzer::buf
1024 样本 Hann FFT，512 hop（48 kHz 时约 21.3 ms 窗、10.7 ms 步进）
  ├─ 总 RMS、bass/high/vocal RMS
  ├─ 5 个显示频带 RMS：20-80 / 80-250 / 250-2000 / 2000-8000 / 8000-24000 Hz
  ├─ 能量慢均线、总能量瞬态均线、噪声底
  └─ mapping::classify_content()：游戏/音乐/人声/安静
  ↓ A 的频带映射、内容模式参数覆盖、幂律曲线、攻放和死区
  ↓ B 开启时调用 SmartEnhancer::process()，对 A 目标输出做乘性增强
  ↓ 目标电平写 AudioHapState.out_l/out_r 原子量
  ↓ vibration_thread 以 max(DLL 输出, 音频输出) 合流并写手柄
```

代码位置：`mod.rs:288-625` 是引擎主循环，`mod.rs:528-573` 是采集、混音和分析入口，`mod.rs:637-715` 是分析结果写输出和遥测；`dsp.rs:207-265` 是 FFT/频带计算，`dsp.rs:267-476` 是映射和 B 层，`dsp.rs:447-476` 是攻放平滑和 `FrameOut`。

### 1.2 `mod.rs`：采集、时序和输出边界

#### 1.2.1 采集和格式解码

`Loopback` 记录设备、采样率、声道数、每样本字节数和采样类型。默认设备路径读取 render 端点的真实混音格式；按 PID 路径使用 `new_application_loopback_client` 并请求 48 kHz F32、WASAPI 自动转换。

`read()` 针对四种格式解码：

- F32：按 IEEE float little-endian 读取。
- I16：除以 `32768.0`。
- I24Packed：三字节有符号扩展后除以 `8388608.0`。
- I32：除以 `2147483648.0`。

这是当前链路的重要稳定性基础。旧版无条件按四字节 F32 解释会把整型设备解成乱码；当前版本已经修复，C 不应重新触碰这一边界。

#### 1.2.2 线程闸门

引擎线程依次处理：退出、马达自检脉冲、音频总开关、暂停键、音频白名单、设备打开/重试、采集读取、占空比保护。关闭、暂停、等待白名单、无设备或长时间无声音时，`set_idle()` 将音频输出归零。

C 只应位于已经存在的 `Analyzer` 处理阶段，不改变：

- WASAPI 采集和白名单语义。
- `AudioHapState.out_l/out_r` 的原子输出边界。
- DLL 与音频的 `max` 合流语义。
- 15 ms 左右的震动线程合流节奏。
- 双马达持续高电平 10 秒后限幅 60% 的占空比保护。

#### 1.2.3 立体声当前处理方式

当前 `mod.rs` 对每个输入帧计算 `l_rms` 和 `r_rms`，得到：

```text
b = l_rms / (l_rms + r_rms)
pan_l = clamp(2b, 0.6, 1.4)
pan_r = clamp(2(1-b), 0.6, 1.4)
```

它只在 `finish_tick()` 把最终 `level_l/level_r` 乘以左右平衡系数。`SmartInput` 没有接收左右声道特征，因此 B 的“音高左右偏置”和实际声道位置是两件事：B 会按低高频给马达偏置，但不会判断一次具体事件来自左声道还是右声道。C 将补上事件级立体声路由，但保留当前总输出平衡作为兜底。

### 1.3 `dsp.rs`：A 的基础分析与映射

#### 1.3.1 频谱和特征

当前 FFT 常量：

- `FFT_SIZE = 1024`。
- `HOP = 512`。
- Hann 窗。
- `RMS_NORM = 0.4330127 * FFT_SIZE`，用于单边谱 Parseval RMS 修正。

每个 1024 样本窗计算：

- `total_rms = sqrt(mean(frame^2)) * 32768`，内部统一为 int16 量纲。
- `bass_rms`：低于用户 `bass_high_hz` 的能量。
- `high_rms`：低于 16 kHz 且不在低频段的能量。
- `voc_rms`：用户 `vocal_low_hz..vocal_high_hz` 的能量。
- 5 个显示频带的 RMS 和峰值保持。

A 的优势是路径短、稳定、无外部模型，频带到双马达的关系直观；缺点是它本质上是“当前能量有多大”的跟随器，不能单独表达一次事件的起点、持续阶段和结束阶段。

#### 1.3.2 能量瞬态、噪声底和内容分类前的状态

总能量慢均线：

```text
energy_avg[n] = 0.965 * energy_avg[n-1] + 0.035 * total[n]
```

总能量瞬态：

```text
transient_raw[n] = clamp(
    (total[n] - energy_avg[n]) / max(energy_avg[n], 80),
    0, 1
)
transient_avg[n] = 0.82 * transient_avg[n-1] + 0.18 * transient_raw[n]
```

噪声底：

```text
noise_floor[n] = 0.995 * noise_floor[n-1]
                 + 0.005 * min(total[n], vocal[n])
```

这些规则对连续输出和背景屏蔽有用，但它们都是总能量级别的单路状态。B 继承了这个 `transient_avg`，没有再把它拆成每个频带的事件证据。

#### 1.3.3 内容三分类

`mapping::classify_content()` 并不是 B 的 `smart.rs` 智能层，而是 A/B 共用的内容配置层。它首先做安静硬判定：

```text
total_rms < 55 → MODE_QUIET
```

其他情况使用占比和总能量瞬态：

```text
bass_ratio  = bass_rms / max(total_rms, 1)
vocal_ratio = vocal_rms / max(total_rms, 1)

music_score = 1.25*bass_ratio + 1.65*transient
asmr_score  = 1.15*vocal_ratio
               + 0.55*(1 - min(2.4*transient, 1))
               - 0.45*bass_ratio
```

上一状态为音乐时给 `music_score + 0.12`，上一状态为人声时给 `asmr_score + 0.12`。只有分差超过滞回边界才切换；否则保持原音乐/人声状态。

这个方法的强弱：

- 强：开销低；有明确的安静出口；滞回能减少临界抖动；适合给“BGM 压制、人声微颤、安静静音”提供慢速配置。
- 弱：它不是训练分类器；只看到低频占比、人声频带占比和一个总能量瞬态，不能可靠区分“低频 BGM 鼓点”和“低频技能冲击”；没有节拍周期、谱平坦度、逐带瞬态或事件历史。
- 结论：C 不替换这个分类器，而是在其上面增加事件级证据。内容分类继续做慢速背景门控，事件状态机负责快速输出。

#### 1.3.4 A 的映射公式

先按 `dual_motor` 决定声源：

```text
双马达分频：src_l = bass_rms，src_r = high_rms
同步模式：  src_l = src_r = total_rms 或 vocal_rms
```

幂律映射：

```text
M(x; T, X, gamma) = 0,                         x <= T
                 = clamp((x-T)/(X-T),0,1)^gamma, x > T
```

其中 `T` 是固定/自适应门槛，`X` 是 `map_max`，`gamma` 由 `punch_pct` 计算：

```text
p = min(punch_pct, 200) / 100
p <= 1: gamma = 0.45 + 0.55*p
p >  1: gamma = 1.0 + 1.2*(p-1)
```

之后乘以 `master`、低/高频灵敏度、BGM 连续输出压制、BGM 鼓点和人声微颤，最后调用：

```text
smooth_signal(current, target, attack, release)
```

上升使用 `attack`，下降使用 `release`，低于死区的电平置零。A 是 C 的最后兜底基线，不能被 C 的新参数改变。

### 1.4 `mapping.rs`：参数和预设边界

`EngineParams` 在 `mapping.rs:16-68` 定义，`EngineParams::load()` 从 `AudioHapState` 原子量快照读取。A/B/C 都应复用现有字段语义：

- 总强度：`master`，0-150%。
- 低/高频量感：`low`、`high`，0-200%。
- 打击对比度：`punch_pct`，0-200%。
- 门槛和底噪：`mask_threshold`、`adaptive_threshold`。
- 内容层：`auto_profile`、`music_mask_pct`、`quiet_mask`、`rhythm_pct`、`texture_pct`。
- 包络层：`attack_pct`、`release_pct`、`dead_zone_pct`。
- 路由层：`dual_motor`、`vocal_only`、`bass_high_hz`、`vocal_low_hz`、`vocal_high_hz`。
- B 智能层：`smart_enabled`、`smart_strength`、`smart_duration`。

预设默认值来自当前 `config.rs` 默认函数和 `mapping::PRESETS[0]`：当前默认预设名是“人声清晰”，不是旧历史文档中的“游戏强劲”。这是设计参数表必须遵守的当前事实。

### 1.5 `resample.rs`：存在但不是当前 v24.60 热路径

`resample.rs` 实现了 Blackman 窗化 sinc 低通、固定 tap 历史和线性插值分数步进，目标注释是任意采样率到 16 kHz，历史上用于 YAMNet 输入。当前必读文件中的 `mod.rs` 和 `dsp.rs` 没有调用它；当前 v24.60 直接使用设备采样率做 1024 点 FFT。因此：

- C 不需要引入重采样。
- C 不应把旧的“16 kHz + 0.96 秒模型窗”当成当前延迟基线。
- 如果未来为离线录音回放统一采样率，可以复用该模块，但不属于本 C 版实施必需项。

### 1.6 `smart.rs`：B 模式逐段剖析

B 的价值在这里。它不是重新做 FFT，而是复用 `dsp.rs` 已经算好的总能量、低频、高频、噪声底、瞬态、映射满量程和有效门槛。

#### 1.6.1 B 的生效闸门

`SmartEnhancer::process()` 在以下任一条件成立时直接返回中性输出：

```text
!enabled
strength_pct == 0
总能量 <= 当前门槛
输入包含 NaN/Inf
```

中性输出是：增益 1、攻放缩放 1、左右偏置 1、点击感 0、`active=false`。这保证了 A 模式和 B 强度 0 都能旁路智能层；现有 `dsp.rs` 单测还验证了逐帧电平相等。

这条闸门是 B 的强点，也是 C 必须继承的安全边界：智能层不应在基础信号没有通过门槛时凭空制造震动。

#### 1.6.2 B 的内部历史状态

B 使用两个状态来源：

1. 32 帧 RMS 环形窗。每帧约 10.7 ms，因此完整记忆约 340 ms。
2. `energy_slow = 0.94*energy_slow + 0.06*total_rms`，作为“晚期/尾巴能量”的廉价代理。

环形窗是固定数组，不分配堆内存；`push_frame()` 只记录有限值并更新慢均线。`window_stats()` 在最多 32 个值上计算均值和标准差。

#### 1.6.3 B 的四个特征

设 `eps=1`，`nf=max(noise_floor,0)`，`span=max(map_max-nf,1)`。

1. **响度/近场线索**

```text
loudness = clamp((total - nf) / span, 0, 1)
```

响度越高，近场分数越高。它简单、稳定，并且与实际马达“强弱”直接相关；但它不是物理测距，只是听觉距离代理。

2. **高频亮度/近场空气吸收代理**

```text
brightness = clamp(high / (bass + high + 1), 0, 1)
```

高频占比越高，声音越亮，近场/尖锐度分数越高。优点是计算极低；局限是只使用 `bass` 与 `high`，中频被折叠到两端，且没有频谱质心、频带斜率和声道信息。

3. **干湿/直达反射代理**

```text
tail   = clamp(energy_slow / (total + 1), 0, 1)
dryness = clamp(0.6*transient + 0.4*(1-tail), 0, 1)
```

强瞬态、短尾巴被当作更“近/干”；平稳且尾巴长的声音被当作更“远/湿”。优点是能在没有多麦克风的情况下给事件做短促/拖尾差异；局限是 `energy_slow` 只是单路慢能量，并不是显式早期反射或混响估计。

4. **单耳 AM 调制深度代理**

```text
am_depth = clamp(std(rms_window) / (mean(rms_window) + 1), 0, 1)
```

包络变化越大，AM 深度越高。它能帮助区分有明显起伏的近场事件和稳定背景；但 340 ms 窗内的标准差不能告诉系统“哪一个子事件刚刚开始”。

#### 1.6.4 B 的距离感和音高融合

距离原始分数：

```text
raw_proximity = clamp(
    0.40*loudness + 0.25*brightness + 0.20*dryness + 0.15*am_depth,
    0, 1
)
```

距离状态使用非对称平滑：向上用 `alpha=0.45`，向下用 `alpha=0.12`。这形成“近感快速建立、慢慢退回”的滞回效果，减少电平边界抖动。

音高分数：

```text
raw_pitch = clamp(0.75*brightness + 0.25*(1-am_depth), 0, 1)
pitch += 0.25*(raw_pitch-pitch)
```

因此 B 的“高低感”主要是高频占比，AM 深度只做轻微修正。

#### 1.6.5 B 输出映射

令 `s = clamp(strength_pct,0,100)/100`，`d=clamp(duration_pct,0,100)/100`，`p=proximity`，`q=pitch`，`t=transient`。

代码中的系数可以写成：

```text
gain         = 1 + s*(-0.45 + 0.75*p)
attack_scale = 1 + s*(-0.20 + 0.60*p)
release_scale= 1 + s*d*(-0.45 + 0.90*p)
left_bias    = 1 + s*( 0.15 - 0.50*q)
right_bias   = 1 + s*(-0.25 + 0.60*q)
click        = 0.18*s*p*t*(1-q)
duration_ms  = clamp(60 + 260*(1-p), 40, 400)
```

`dsp.rs` 不是把点击感独立加到零输出，而是乘到基础目标上：

```text
t_l = clamp(base_l * gain * left_bias  * (1 + click), 0, 1)
t_r = clamp(base_r * gain * right_bias * (1 + 0.35*click), 0, 1)
```

再把攻、放速率乘以 `attack_scale`、`release_scale`，最后进入原有 `smooth_signal()`。

这解释了 B 实机为什么会比 A 更好：它不是简单整体放大，而是在同一个基础事件上同时改变强弱、起手速度、收尾速度、左右马达比例和近场点击感。当前单测也验证了：近场输入的增益、起手、收尾和点击感均应强于远场输入。

#### 1.6.6 B 已验证的强点

以下能力应在 C 中原样保留：

| B 环节 | 已有强点 | C 的继承方式 |
|---|---|---|
| 中性旁路 | `enabled=false` 或强度 0 返回全 1 系数；不凭空制造输出 | C 始终先跑 B；C 层失败或关闭时直接取 B 目标 |
| 多线索融合 | 响度、亮度、干湿、AM 深度共同估计近感 | 保留 B 四项权重和状态平滑，C 只增加事件级修正 |
| 近远手感 | 近场更猛、更快、更短；远场更轻、更拖尾 | C 事件包络和事件增益继续使用 `proximity` |
| 高低手感 | 低音偏左、高音偏右 | C 叠加频带事件权重，不取消 B 的左右偏置 |
| 轻量实时 | 不重算 FFT；32 帧固定数组；无模型 | C 复用 FFT 结果，新增固定环形缓冲和少量标量运算 |
| 异常安全 | NaN/Inf 立即中性回落 | C 增加同样的有限值检查，单独降级到 B |
| 实机可调 | `smart_strength_pct`、`smart_duration_pct` 语义已经存在 | 旧字段不变；新增 C 层参数单独命名并可关闭 |

用户已确认 B 实机效果比 A 好；上表中的“已验证”指用户实机反馈和现有代码单测共同支持的方向，不代表已经完成系统性 C 对照实验。

#### 1.6.7 B 的代码级短板

1. **事件颗粒度粗**：B 输入只有总量、bass、high、noise floor、总瞬态和阈值。它没有“事件开始/持续/结束”状态，也没有逐频带事件分数；`click` 只是当前帧乘性系数。
2. **总能量瞬态会把不同频段混在一起**：`transient_avg` 来自总 RMS 的慢均线，低频鼓点、高频技能音和宽带爆发共享同一个爆发度，无法仅靠该值分配不同波形。
3. **没有节拍/重复模式识别**：BGM 鼓点和连续技能都可能产生相似的瞬态；当前音乐判断是低频占比 + 总瞬态的启发式，没有周期稳定性证据。
4. **没有事件级声道特征**：左右声道 RMS 只在最终电平阶段做一次平衡；`SmartInput` 不含 L/R 或逐频带声道能量，因此 B 的左右偏置不是声源空间定位。
5. **B 不能救活门槛以下的基础输出**：`total <= threshold` 直接中性返回；后续乘性映射也不会凭空产生输出。这是有意的安全策略，但会漏掉“总量不高、频带很集中、实际应有一记轻触”的事件。
6. **时长是估算值，不是事件调度**：`duration_ms` 只写遥测；实际输出持续时间仍由目标电平和 `release` 平滑决定。它不能保证每次事件都按估算时长结束。
7. **特征暖机和时间尺度固定**：32 帧约 340 ms 的窗口适合稳健估计，但短于窗口的快速连击会被统计量混合，导致事件边界不清。
8. **输出饱和会压缩层次**：B 的增益、偏置、点击感都是乘性系数，经过 `clamp(0,1)` 后高电平事件可能都挤在顶部；B 解决了“太轻”的问题，但没有专门的峰值/headroom 管理。
9. **当前 FFT 热路径仍会构造 `frame` 和复数 `Vec`**：这不是 B 算法本身的缺陷，但 C 不能在此基础上继续增加每帧堆分配；实施时应预分配工作区并用基准测试确认。

### 1.7 A/B 文件职责总表与 C 接入位置

| 文件 | A 现状 | B 现状 | C 的设计接入点 |
|---|---|---|---|
| `mod.rs` | 采集、混音、pan、线程闸门、输出 | 同 A | 只需把模式参数放入 `EngineParams`；不改采集边界、输出原子量和 pan 一次性应用 |
| `dsp.rs` | FFT、RMS、分类、曲线、攻放 | 在映射后调用 `SmartEnhancer` | 同一 FFT 频谱中计算特征；先得到 A、B 目标，再把 C 候选和 B 目标混合 |
| `mapping.rs` | 参数快照、幂律、攻放、内容分类 | 提供 B 所需字段和分类 | 加模式及 C 参数快照，保持 A/B 字段和默认值兼容 |
| `resample.rs` | 当前路径未调用 | 当前路径未调用 | 不作为 C 依赖，避免模型窗延迟和无谓 CPU |
| `smart.rs` | 旁路 | 距离/音高/时长无模型增强 | 保留原 B 公式和状态；C 使用 B 的 `SmartOutput`，异常时旁路 C |
| `config.rs` | 旧字段默认值、`smart_enabled=false` | `smart_enabled=true` + 两条强度滑条 | 新模式字段仅在显式设置时生效；旧配置继续按 `smart_enabled` 映射 A/B |
| `state/audio_state.rs` | A/B 原子量和遥测 | B 原子量和遥测 | C 参数状态独立；错误回退 C→B，不清掉 B 状态 |
| 音频 egui 页 `gui/audio_page.rs` | A/B 切换 | 现有 B 滑条和遥测 | 增加 C 选项、C 升级强度和 BGM 状态；本轮只列实施建议 |

事件级声像定位不是 C v1 的接入点：`Analyzer::process()` 当前不接收 pan，`finish_tick()` 已统一乘一次 pan；C v1 只新增频带到马达的路由，不重复施加立体声 pan。

---

## 2. C 模式目标定义

### 2.1 定位

C 的一句话定义：

> **B 的近远/高低/时长感知不变，再增加“这是不是一记独立事件、属于哪个频段、是否只是规律 BGM 鼓点、来自哪一侧”的快速判断。**

C 不承诺仅凭混音音频准确说出“这是某个 DNF 技能名”。在没有专用标注数据和模型的条件下，C 的可验证目标是“事件是否独立、强弱是否合适、节奏是否规律、频谱和空间如何分配”，而不是语义标签幻觉。

### 2.2 量化目标与验收线

下表是实施前目标，全部需要按第 6 节实验验证：

| 维度 | C 目标 | 对照和说明 |
|---|---:|---|
| 软件起震延迟 | P50 ≤ 20 ms，P95 ≤ 25 ms，P99 ≤ 35 ms | 以输入事件标注到 `out_l/out_r` 首次超过 0.10 的时间；C 相比 B 增量不超过 3 ms。待验证 |
| 端到端延迟 | ≤ 50 ms（P95） | 含 WASAPI 缓冲和转子马达物理起振；软件与物理部分分开记录。待验证 |
| 独立事件 F1 | C 相比 B 提高至少 10 个百分点 | 事件容差 ±50 ms；DNF 音效和合成片段分开统计。待验证 |
| BGM/语音误触发 | 相比 B 每分钟误触发数下降至少 30% | 不能以“完全静音”换取事件召回；持续声和真实技能声分开统计。待验证 |
| 连续事件分离 | 事件间隔 ≥80 ms 时，至少 90% 能被分成两个峰 | B 作为主基线；待验证 |
| 近远单调性 | 近场平均输出 > 远场；近场起振时间更短、收尾时间更短 | 继承 B 的现有单测方向；C 不得破坏。 |
| 低/高频路由 | 低频事件 L/R 峰值比、高频事件 R/L 峰值比均较 B 不差 | 同时计入声道 pan，避免用单一频率样本误判。待验证 |
| 输出层次 | P95-P25 电平动态范围相对 B 不降低，顶部饱和占比不增加超过 5% | 用 headroom 公式控制。待验证 |
| CPU | 音频线程平均 <5% 单逻辑核；C 相比 B 增量 ≤2 个百分点（约 ≤20 ms CPU / 秒输入音频） | 具体机器和编译模式待验证；纯 Rust，不使用 GPU。 |
| 单 tick 计算 | P99 <2 ms；不等待 GUI、磁盘或网络 | 10 ms 采集 tick 下的硬实时预算。待验证 |
| 内存/分配 | C 新增固定状态不超过 64 KiB；热路径零新增堆分配 | 当前 `dsp.rs` 已有工作区分配，C 实施时应至少不增加分配；是否预分配现有工作区另列 PR。 |
| 长时间稳定性 | 1 小时无 NaN、无状态泄漏、无马达持续满幅；C 故障可自动退 B | 待验证 |

### 2.3 “更强”具体强在哪里

- **BGM 判定准确性**：B 的近远判断保留；C 不把 B 的距离估计改成未经验证的模型，而是增加持续性、和声性、重复性、节拍、谱稳定和新颖度证据，减少“BGM 鼓点=普通事件”的混淆。
- **BGM 筛选层次**：C 不做“一刀切静音”，而是分别压持续和声底、规律鼓点和非周期突发；`MUSIC_LOCK` 只在多证据一致时强压，技能/受击瞬态走 escape。
- **事件颗粒度**：B 的当前帧乘性增强升级为 onset、hold、release、refractory 四阶段；同一段连续声不再被每帧当成新事件。
- **手感层次**：保留 B 的近/远、高/低、短/长，增加事件峰值、低/高频事件权重、宽带事件逃逸和左右声道位置。
- **延迟**：不引入 0.5-1 秒语义模型窗；事件通路只看当前 1024 窗和历史短状态，目标是和 B 同级，增量不超过 3 ms。
- **可解释性**：每次输出都能在遥测中解释为 `b_proximity`、`event_score`、`C_music_raw`、`C_conf`、`beat_lock`、`music_gate`、`event_class`，调参不依赖黑盒模型。

---

## 3. 候选方案对比

### 3.1 路线一：B 基础 + 多带瞬态 + 节奏锁 + 事件状态机（推荐）

#### 做法

复用 B 的 `SmartEnhancer` 完整结果；在同一个 FFT 帧中利用已经存在的频谱 bin，再计算：

- 五带或工作频带的逐带包络。
- 逐带谱通量/超阈值余量。
- 谱平坦度、频带对比度和事件新颖度；马达路由用频带权重，继续复用当前全局 L/R pan。
- 约 1.5-2 秒低频包络自相关，识别稳定周期 BGM。
- onset → hold → release 的事件状态机和短暂 refractory。

C 的新增量只在 B 输出上做有界事件增强，不替换 B 的距离/音高/时长公式。

#### 优点

- 最贴合“B 的下一代”定位，B 结果天然是可回退基线。
- 不增加模型文件、不引入 ONNX Runtime、不引入 GPU。
- 事件通路可以在一个 FFT 帧内完成，延迟可控。
- 复用 `rustfft`、频带 bin、`AudioHapState` 和当前输出边界。
- 每条规则可测、可解释、可在合成音频上验证。

#### 局限与风险

- 只能做声学事件级别的判断，不能保证区分枪声、爆炸和 DNF 某个技能的语义。
- 自相关节奏锁需要历史窗口；刚启动或节拍变化时会有约 1-2 秒暖机，必须让快速事件通路不依赖它。
- 频谱平坦度和周期性对某些宽带 BGM/技能音的边界仍可能混淆。
- 当前 FFT 热路径已有 `Vec` 构造，实施时需要基准和可能的预分配工作区。

#### CPU 预算（目标，待验证）

- 逐帧新增：五带差分、24 组中值/相似度以及固定环形状态；目标摊销 ≤0.15 ms/帧。
- 每 100 ms 执行一次节拍 lag 搜索，目标摊销 ≤0.20 ms/10 ms tick。
- 谐波扫描、事件状态机和门控映射合计目标 ≤0.05 ms/帧。
- C 相对当前 B 的平均增量目标 ≤2% 单逻辑核（约 20 ms CPU/s 输入音频），P99 tick 计算 <2 ms。以上都是工程预算估算，必须由 release benchmark 验证；若超预算，先降低 lag 候选数/频谱组数，再移除心理声学辅助层，不得侵入快速事件通路。

以上不是实测结果；需要在实施 PR 的 release 构建和用户目标机器上测量。若达不到预算，先关闭节奏相关计算，仍保留逐带事件通路。

### 3.2 路线二：B 基础 + HPSS + 自适应谱底差分（轻量最小路线）

#### 做法

保留 B 的距离/音高层；对 24 个对数频谱组维护因果中值背景 `H_g` 和较慢的分位/指数谱底 `Q_g`，将当前谱相对稳定背景的增量作为候选技能事件：

```text
Q_g[n] = min( Q_g[n-1] + eta_up*max(X_g[n]-Q_g[n-1],0),
              Q_g[n-1] + eta_down*min(X_g[n]-Q_g[n-1],0) )
D_g[n] = max(0, X_g[n] - max(H_g[n], Q_g[n]))
E_sparse = sum_g D_g[n] / (sum_g X_g[n] + eps)
```

`eta_up < eta_down`，使背景谱底能较快跟随上升、较慢释放；HPSS 的时间中值负责分离稳定背景与瞬态。BGM 置信度只由“稳定谱底比例 + 重复相似度”组成，不运行节拍搜索、谐波扫描和心理声学后处理。

#### 优点

- 完全纯 DSP，无模型/权重/训练集。
- 只有固定谱组、单一背景状态和因果中值，CPU 和内存预算最低。
- BGM 频谱长期稳定时能较快压住持续层；换曲可用谱新颖度重置。
- 结构简单、实现风险低，适合低端机器或“省电档”。

#### 局限与失效场景

- 纯钢琴、管弦和电影配乐不一定频谱稳定或周期重复，漏筛会明显高于路线一。
- 持续技能音、吟唱或引擎声可能被吸收到谱底；如果 `eta_up` 过慢，换曲后旧背景拖留，过快则持续技能也被吞。
- 没有节拍一致性和谐波结构交叉验证，连招音效可能与电子 BGM 混淆。
- 没有单独的心理声学掩蔽控制，输出层次主要依赖频带能量。

#### 预算与决策

设计目标：每帧仅 24 组 × 小窗口中值/差分，新增状态低于 8 KiB，CPU 增量目标 ≤0.5% 单逻辑核；均待目标机器实测。该路线适合作为路线一的性能降级档，不作为默认 C，因为用户要求重点提升复杂 BGM 的筛选率。

### 3.3 方案对比表

| 维度 | 维度 | 路线一：多证据事件 DSP（推荐） | 路线二：HPSS + 自适应谱底差分 |
|---|---|---|
| B 继承程度 | 直接复用 B 结果，最高 | 直接复用 B 结果 |
| BGM 覆盖 | 循环/规律/和声/稳定多证据 | 主要覆盖稳定、重复的谱底 |
| 钢琴/管弦等非强节拍 BGM | 谐波、HPSS、重复/稳定度互补，仍待验证 | 较易漏筛或误吸收持续技能 |
| 事件颗粒度 | 高：逐带、状态机、周期锁、事件逃逸 | 中：谱差分和基本 onset |
| 快速延迟 | 快速通路一个当前 FFT 帧内 | 一个当前 FFT 帧内 |
| CPU | 低到中，慢通道节流运行 | 最低，固定谱组运算 |
| 内存 | 固定小环形缓冲，目标 <64 KiB | 目标 <8 KiB |
| 参数敏感度 | 中高，需消融/素材验证 | 中，谱底时间常数较敏感 |
| 失败模式 | 证据冲突时进 UNKNOWN、持续层按上限压制、突发事件逃逸 | 换曲拖尾、持续技能被吸收到谱底 |
| 适配现有结构 | 扩 `Analyzer`/独立 C 状态/遥测 | 扩 `Analyzer`/谱底状态 |
| 推荐场景 | 默认 C，追求 BGM 筛选更完整 | 低 CPU 备选档 |
| 推荐 | **推荐 C v1** | 可做低功耗降级路线 |

---

## 3.4 C 模式招牌能力：纯算法 BGM 筛选

### 3.4.1 目标和总体原则

C 的招牌能力不是“把所有声音都变得更响”，而是从游戏混音流中估计一个**持续背景层**，把它压低，再把突发、宽带、非重复的技能音效/受击音/打击音留在主导位置。这里的 BGM 是声学意义上的背景音乐，不承诺识别歌曲名称，也不把“低频”直接等价为 BGM。

```text
输入：游戏混音频谱和短期历史
  ↓
背景证据：和声性 + 重复性 + 节拍规律 + 频谱稳定 + HPSS 背景掩膜
事件证据：谱通量 + 宽带度 + 频带新颖度 + 非周期性
  ↓
带滞回的 BGM 置信度与状态机
  ↓
持续成分强抑制；规律鼓点中等抑制；高置信突发事件逃逸
  ↓
叠加 B 的近远/高低/时长感知和 C 事件包络
```

设计原则：

1. **纯算法**：不训练、不使用神经网络、不读取模型权重、不依赖外部数据集；只使用当前 FFT、固定历史环形缓冲和经典 MIR/DSP 统计量。
2. **因果实时**：在线版本只使用当前帧和过去帧；所有需要中心窗口的离线算法改写为过去窗口或只用于慢速背景估计。C 不能为了“看清 BGM”延迟技能音。
3. **多证据才锁定**：谱平坦度、低频、节拍中的任何一项都不能单独宣布“这是 BGM”。至少需要重复性/节拍与和声/稳定性中的多项同时支持。
4. **背景和事件分路**：BGM 过滤作用于持续底层；事件分路仍保留快速通道。这样不确定时不会把一次真正技能音跟着 BGM 一起切掉。
5. **自适应重收敛**：换曲、交叉淡入、静音和新场景要清空过时节奏/重复历史，避免旧歌的“音乐锁”污染新歌。

### 3.4.2 B 当前 BGM/人声/安静判定的基准

B 的内容层在 `mapping::classify_content()`，不是一个独立的音乐识别器：

```text
quiet：total_rms < 55
music_score = 1.25*bass_ratio + 1.65*transient_avg
asmr_score  = 1.15*vocal_ratio
               + 0.55*(1-min(2.4*transient_avg,1))
               - 0.45*bass_ratio
```

上一状态是音乐/人声时分别加 0.12 滞回偏置，分差不足则保持旧状态。`dsp.rs` 再按模式改变门槛、连续输出、节奏力度和人声纹理。B 的 `smart.rs` 还用四个线索估计距离：

```text
loudness、brightness、dryness、am_depth
```

它们会改变 `gain`、`attack_scale`、`release_scale`、左右偏置和乘性 `click`，但没有单独输出 BGM 置信度，也没有显式的背景/事件分量。

B 的优点：

- 计算极轻，约 10 ms 级 tick 可直接运行。
- 安静有硬出口，音乐/人声切换有基础滞回。
- 低频和瞬态确实能把很多 BGM 识别成音乐态，用户实机确认 B 的整体手感优于 A。
- B 的距离、音高、短长差异化已经能让同一基础声音产生层次，不应被 C 重写。

B 的 BGM 短板必须逐维补强：

| 维度 | B 当前实现 | C 的补强方向 |
|---|---|---|
| 持续性 | 只有 `energy_avg` 和约 340 ms RMS 窗 | 维护数秒级低成本背景历史和稳定性；静音/换曲立即重置 |
| 和声性 | `vocal_ratio`、`bass_ratio`，没有基频/谐波结构 | 加谐波性分数、谱质心稳定、HPSS 谐波掩膜 |
| 节奏规律 | `transient_avg` 只表示“最近是否爆发” | 加 onset 包络自相关、周期候选一致性和节拍锁定 |
| 重复性 | 没有相似帧/重复段比较 | 加 REPET-SIM 风格的滞后相似度，识别重复 BGM 背景 |
| 事件分离 | 总能量瞬态混在一起 | 加逐带谱通量、频带新颖度、宽带/打击乐证据 |
| 人声保护 | 用一个 vocal 频带和 AM 间接判断 | 单独维护 voice-like 抑制分支，不能把语音当 BGM 依据 |
| 换曲/静音 | B 的慢状态可能留下旧上下文 | 新颖度/静音触发解锁，短时间内只保留 B 事件通路 |
| 输出粒度 | `smart_duration_ms` 是遥测估算，未调度事件结束 | C 用背景/事件双分量和事件状态机真正控制释放 |

因此 C 的目标不是凭空宣称“100% 识别 BGM”，而是让 BGM 筛选在**持续性、和声性、规律性、重复性、事件对比度和重收敛**六个维度都比 B 有证据增量。

### 3.4.3 候选算法逐项评估

下表中的 CPU 是相对当前单个 1024 点 FFT 的设计级预算，不是实测结果；所有“有效性”都必须用第 6 节素材和用户机器 benchmark 验证。

| 算法 | 核心信号 | 游戏混音有效性 | CPU/内存预算 | 参数敏感度 | 主要失效场景 | C v1 决策 |
|---|---|---|---|---|---|---|
| HPSS / 中值滤波谱分离 | 时间轴中值保留和声，频率轴中值保留打击 | 对持续和声 BGM 与突发打击有帮助；适合把“稳定横向谱线”和“瞬时竖向谱线”分开 | 5 带/分组后固定小窗口，低；若逐 bin 中值需排序，约为 FFT 的数倍以内；固定环形缓冲 | 时间/频率窗口、掩膜幂次敏感 | 纯钢琴既有和声又有打击，电子鼓与技能音相似，混响把打击拖成长尾 | **采用，作为背景/事件证据，不单独判定** |
| REPET / REPET-SIM | 过去帧与滞后帧相似，估计可重复背景 | 对循环 BGM、稳定伴奏强；能压低持续重复层 | 5-24 个谱组 × 85-1000 ms 滞后，低到中；2 s 环形缓冲约几 KB | 相似度阈值、最小重复次数、换曲重置阈值 | 无循环的电影式配乐、BGM 与技能同节奏、短片段不足一个周期 | **采用，做慢速强证据** |
| 谱平坦度/质心/滚降 | 频谱像噪声还是音调，亮度和频率重心 | 低成本区分窄带和宽带；可辅助纯音 BGM、噪声技能和高频冲击 | 已有 FFT 顺手累加，极低 | `eps`、频段、归一化和阈值敏感 | 铃声、钢琴、合成器都可能窄带；压缩噪声会抬高平坦度 | **采用，权重低** |
| 谱通量/新颖度 | 当前频谱相对上一帧的正向变化 | 对技能起音和受击音很有效，是 BGM 逃逸的重要证据 | 已有相邻帧，可低成本完成 | 白化/对数、阈值、带权重敏感 | BGM 鼓点本身通量高；编码 pre-echo 可能制造小峰 | **采用，作为事件证据** |
| 基频追踪/谐波性 | 自相关或谐波模板命中 | 纯钢琴、弦乐、电子和声等 BGM 有帮助；能证明“有持续音高结构” | 粗略 36 个基频候选×3-6 次谐波，低到中；不建议逐样本 YIN | F0 范围、谐波数、容差敏感 | 角色语音也有强谐波；技能吟唱可能被误认为音乐 | **采用，不能单独门控** |
| 节拍/韵律建模 | onset 包络自相关、周期一致性 | 对电子 BGM、鼓点和循环节奏强；能把规律鼓点与随机技能区分一部分 | 2 s 历史，每 100 ms 搜索 85-1000 ms，低到中 | lag 范围、相关阈值、锁定次数敏感 | 连续技能、连招音效也可能规律；变速/切拍 BGM 会掉锁 | **采用，慢速锁定** |
| NMF（无训练迭代） | `V≈WH`，在线分解谱源 | 理论上能拆出重复伴奏和突发源，混合密集时有潜力 | 每帧 `O(FK)`，多次乘法更新；K=4-8、3-10 次迭代会明显高于当前链；还需缓存 W/H | 秩 K、迭代数、初始化和平滑高度敏感 | W/H 交换、收敛慢、技能和 BGM 同频无法分离；突发时来不及收敛 | **不进 C v1，作为离线/未来实验** |
| CQT/多分辨率变换 | 对数频率分辨率，适合音高与和声 | 对纯钢琴、弦乐和跨八度和声比线性 FFT 更稳；低频长窗、高频短窗可兼顾 | 额外滤波器组或多 FFT；CPU 和代码复杂度中高 | bins/oct、最低频、窗长敏感 | 采样率/设备变化、瞬态延迟、实现和测试面变大 | **不新增 C v1；用 FFT 分组近似** |
| 心理声学掩蔽 | 以强背景频带抬高弱成分阈值 | 能避免在听觉上已经被 BGM 掩蔽的细碎底层持续震；适合输出后处理 | 频带级 Bark/临界带统计，低 | 掩蔽斜率、扩散和释放时间敏感 | 真实技能也可能和 BGM 同频，过强会漏掉关键音效 | **采用为保守后处理，不作 BGM 分类器** |
| Ogg/MP3 编码伪迹 | pre-echo、块噪声、高频 warble 等 | 当前是 WASAPI 解码后的系统混音，缺少编码元数据；伪迹不稳定，不能可靠证明 BGM | 观察伪迹本身成本低，但特征收益不可证明 | 编码器、码率、重采样和设备链敏感 | 无损/本地 PCM、不同播放器、游戏内音频压缩方式差异极大 | **不采用为判定特征** |

#### 3.4.3.1 HPSS/中值滤波的在线化细节

对当前 FFT 功率谱 `P[k,n]` 取对数：

```text
L[k,n] = ln(1 + P[k,n]/eps)
```

`ln(1+P/eps)` 保证输入非负，避免静音或小于 1 的线性功率让中值比值失去意义。标准 HPSS 常用沿时间和频率的中值滤波。实时 C 不使用未来帧，改成过去样本：

```text
W_t = c_hpss_time_frames
W_f = c_hpss_freq_groups
H[k,n]  = median { L[k,n-j] | j=0..W_t-1 }
Pp[k,n] = median { L[k-m,n] | m=-W_f/2..W_f/2 }

H_p = H[k,n]^p
P_p = Pp[k,n]^p
M_h[k,n] = H_p / (H_p + P_p + eps)
M_p[k,n] = 1 - M_h[k,n]
```

其中 `p=c_hpss_mask_power`，默认 1.0；`p>1` 让掩膜更硬，`p<1` 让背景/事件分离更柔和。这里 `c_hpss_freq_groups` 默认 7，表示频率中值窗口宽度。`M_hpss` 是 24 个对数谱组上按当前功率加权的 `M_h` 平均，`M_p_hpss=1-M_hpss`；不要把每个 bin 的掩膜直接等权平均，否则静音频段会放大噪声。

`H` 是“横向稳定/和声背景”证据，`M_p` 是“竖向/瞬态打击”证据。`W_t=9` 帧约 96 ms、`W_f=7` 个相邻谱组是实时起点；C 不需要对每一个 1024 bin 做复杂排序，可以先把谱聚合到 24 个对数频率组再做固定小数组中值。

重要限制：中心中值滤波会使用未来数据，若直接照搬离线 HPSS 会增加 look-ahead；C 只能把过去中值用于慢速背景置信度，事件通路仍使用当前帧谱通量。纯钢琴的起音可能被 `M_p` 识别为打击，不能据此直接放行或压制。

#### 3.4.3.2 REPET/REPET-SIM 的轻量化

令 `q_n` 是当前帧的 24 维、非负、L2 归一化对数谱组向量。对候选 lag `l`（单位 hop 帧）比较当前谱和若干历史重复副本：

```text
l_min = max(ceil(c_repet_lag_min_ms*sample_rate/(1000*HOP)), ceil(FFT_SIZE/HOP))
l_max = floor(c_repet_lag_max_ms*sample_rate/(1000*HOP))
R_l,j[n] = exp(-(1-cos(q_n, q_{n-j*l}))/c_repet_sigma), j=1..c_repet_repeats
R_rep[l] = clamp(mean_j(R_l,j)-0.5*std_j(R_l,j),0,1)
l* = argmax_{l_min..l_max} R_rep[l]
R_rep = R_rep[l*]
```

所有索引必须满足 `n-j*l >= 0` 且落在固定 3 秒 ring buffer 内；任一 lag 不足 `c_repet_repeats=3` 个独立副本时该 lag 标为 unavailable。该设计要求历史长度至少覆盖 `c_repet_repeats*c_repet_lag_max_ms`；默认重复历史应设为 3000 ms，与默认 1000 ms 最大 lag、3 个历史副本相容。未积累到 3 个分离副本时 `R_rep` 标为 unavailable，不把“尚无历史”当成“确定非音乐”。减去半个标准差用于抑制偶然相似。

候选最优 lag 还需在连续 `c_rep_update_ms` 慢通道更新间稳定：

```text
lag_delta = abs(l*_n-l*_{n-1}) / max(l*_{n-1},1)
lag_stable = lag_delta <= c_repet_lag_stability_pct/100
```

REPET 适合循环 BGM，但不能保证无循环配乐；它也不能证明相似段不是连续技能。故它只作为慢速证据，并必须和和声/稳定/节拍至少两项一致。

#### 3.4.3.3 谐波性、谱稳定和语音保护

粗略谐波性不需要完整 YIN。对 `c_harmonic_f0_min_hz..c_harmonic_f0_max_hz` 的 36 个对数候选基频 `f`，对每个谐波取频率容差窗口中的峰值：

```text
k(m,f) = round(m*f*FFT_SIZE/sample_rate)
P_h(m,f) = max P[k]，其中 |k-k(m,f)| <= c_harmonic_tolerance_bins
Harm(f) = sum_{m=1..c_harmonic_partials} (1/sqrt(m))*P_h(m,f)
          / (sum_{k in valid_bins} P[k] + eps)
H_harm = clamp(max_f Harm(f), 0, 1)
```

只累加 `k < FFT_SIZE/2` 的有效单边谱 bin；越出 Nyquist 的谐波跳过，不得索引越界。再维护约 200 ms 的谐波性和谱质心稳定度：

```text
centroid[n] = sum_k(freq_k*P[k,n]) / (sum_k P[k,n]+eps)
S_centroid = exp(-abs(centroid[n]-median_recent) / sigma_centroid)
```

语音也常有谐波，因此增加 vocal 比例和调制保护。C 不读取 `SmartEnhancer` 私有的 AM 统计量，而是自己用固定 32 帧的 vocal-band RMS 环形窗计算：

```text
am_depth_c = clamp(std(vocal_recent)/(mean(vocal_recent)+eps), 0, 1)
V_voice = clamp(
    0.50*vocal_rms/(total_rms+eps)
  + 0.30*am_depth_c
  + 0.20*(1-W_low),
  0, 1
)
```

`V_voice` 高时，C 不把“有谐波”直接加成 BGM；语音是否跟随仍由 B 的人声/细节策略决定。角色吟唱、合唱和钢琴都可能同时满足音乐和语音线索，必须允许“不确定”状态。

#### 3.4.3.4 节拍和韵律置信度

用低频包络和逐带谱通量构造 onset 序列：

```text
o[n] = max(0, 0.55*x_0[n]+0.45*x_1[n]
              -0.55*x_0[n-1]-0.45*x_1[n-1])
```

对候选周期 `l` 做归一化自相关：

```text
A(l) = sum_{n=N-l..N-1} o[n]*o[n-l]
       / sqrt(sum o[n]^2 * sum o[n-l]^2 + eps)
R_beat = max_{l_min<=l<=l_max} A(l)
```

令 `l*` 为使 `A(l)` 最大的候选 lag；旁瓣比取排除 `l*±1` 帧后的最大相关峰：

```text
side_lobe_ratio = max_{l not in [l*-1,l*+1]} A(l) / max(A(l*),eps)
period_stability = 0, if no previous valid l*
period_stability = clamp(
    1 - abs(l*_n-l*_{n-1})
        / max((c_beat_stability_pct/100)*l*_{n-1},1),
    0, 1), otherwise

B_beat = clamp(
    0.60*R_beat
  + 0.25*period_stability
  + 0.15*(1-side_lobe_ratio),
  0, 1)
```

如果一段技能连击也很规律，`B_beat` 可能高；此时宽带度、谱新颖度和 HPSS 打击比例通常也高，C 应把它当“规律但可能是事件”的不确定状态，而不是直接强压。

#### 3.4.3.5 NMF 为什么暂不采用

无训练 NMF 仍然是纯算法：对非负谱 `V` 迭代求 `W,H`，典型乘法更新可写为：

```text
H <- H * (W^T V) / (W^T W H + eps)
W <- W * (V H^T) / (W H H^T + eps)
```

它不需要预训练权重，但“无训练”不等于“零成本”：

- 每帧或每个短窗要做多轮矩阵乘法。
- 在线初始化决定收敛方向，`W/H` 存在成分置换和漂移。
- BGM 和技能同一频带时，NMF 没有语义依据保证哪一列是“背景”。
- 用较少迭代可减 CPU，却会变成不稳定的即时猜测；用较多迭代又会延迟和占用超预算。

因此 NMF 可作为离线研究基线或未来“慢速背景估计”实验，但 C v1 不把它放入实时输出回路。采用 HPSS/REPET 的可解释统计量，能更直接满足当前 B→C、低 CPU 和快速回退要求。

#### 3.4.3.6 CQT/多分辨率和编码伪迹的边界

CQT 对和声、八度关系和纯钢琴有优势，但当前代码已经有 1024 点 FFT 和固定 5 带能量。若直接新增 CQT，会引入额外滤波器组、窗长、采样率和延迟边界；C v1 先用 FFT bin 做对数频率聚合和粗谐波模板近似。

Ogg/MP3 的 pre-echo、块边界噪声、高频衰减和 warble 不能作为可靠的 BGM 标签：WASAPI loopback 捕获的是解码后系统混音，代码拿不到原始编码格式或码率；不同播放器、游戏资源压缩器和重采样器会制造不同伪迹。C 只通过对数压缩、短窗中值和谱通量滞回降低编码伪迹误触发，不把伪迹当成 BGM 证据。

### 3.4.4 推荐组合：多证据 BGM 置信度

#### 3.4.4.1 每帧特征

在当前 FFT 中顺手计算以下归一化量，全部限定在 `[0,1]`：

- `M_hpss`：24 个对数谱组的谐波背景掩膜加权均值；启动未暖机时标为 unavailable。
- `R_rep`：REPET-SIM 滞后重复分数；不足 3 个重复样本时标为 unavailable。
- `H_harm`：粗谐波性。
- `B_beat`：节拍相关和周期稳定分数；尚无稳定候选时为 0。
- `N_narrow`：谱平坦度反向分数，窄带/音调性越强越高。
- `S_stable`：谱质心、频带比例和能量的短时稳定度。
- `M_b`：B 当前 `MODE_MUSIC` 的先验，音乐态为 1，否则为 0。
- `E_event`：突发事件分数。
- `V_voice`：基于 C 自己的短期能量环形窗估计的语音保护分数；不依赖 `SmartEnhancer` 私有状态。
- `N_novel`：当前帧相对短期背景的谱新颖度。

谱平坦度：

```text
flatness = exp(mean_k(ln(P[k]+eps)))/(mean_k(P[k])+eps)
wide     = clamp(
    (flatness-c_flatness_low)
    / max(c_flatness_high-c_flatness_low, eps),
    0, 1)
N_narrow = 1-wide
```

默认 `c_flatness_low=0.18`、`c_flatness_high=0.55`，只作为设计起点；它们应在纯钢琴、电子合成、宽带技能和编码伪迹素材上重新标定。`P[k]` 限定在 20 Hz 到 16 kHz 的有效 bin，避免 DC 和极高频设备噪声改变 flatness。

谱新颖度使用短期背景谱 `q_bg`：

```text
q_n   = normalize([ln(1+P_g/eps)]_g)
q_bg  = normalize((1-alpha_n)*q_bg_prev + alpha_n*q_n)
sim_bg = dot(q_n, q_bg)
N_novel = clamp(
    (c_novelty_high-sim_bg)
    / max(c_novelty_high-c_novelty_low, eps),
    0, 1)
```

默认 `alpha_n=0.08`、`c_novelty_low=0.82`、`c_novelty_high=0.98`；`sim_bg` 越低表示当前谱型偏离近期背景越多。`q_g` 是 24 个对数谱组，不对原始 1024 个 bin 做重复归一化。

谱稳定度可以用短时均值和 MAD 计算：

```text
S_stable = 1 - clamp(
    0.5*MAD(energy_recent)/mean(energy_recent)
  + 0.5*MAD(centroid_recent)/max(mean(centroid_recent),eps),
  0, 1)
```

事件分数使用 C 的快速证据：

```text
E_event = clamp(
    0.35*F_norm
  + 0.25*wide
  + 0.20*M_p_hpss
  + 0.20*N_novel,
  0, 1)
```

`wide` 是谱平坦度映射出的宽带分数，`M_p_hpss=1-M_hpss`。事件分数高并不等于“技能”语义，只代表不应被背景门控一刀切。

#### 3.4.4.2 背景、事件和不确定性分数

先合成持续背景证据。启动历史不足时，对 unavailable 特征重新归一可用权重，不把暖机缺数据算成 0 置信：

```text
w     = [0.20,0.20,0.20,0.18,0.12,0.06,0.04]
x     = [M_b,M_hpss,R_rep,B_beat,H_harm,N_narrow,S_stable]
avail = [1,M_hpss_ready,R_rep_ready,B_beat_ready,1,1,S_stable_ready]
C_bg  = sum_i(w_i*avail_i*x_i) / max(sum_i(w_i*avail_i),eps)
```

HPSS、REPET、节拍和稳定度各自的 `ready` 只在对应历史样本足够后为 1：HPSS 需时间中值窗完整；REPET 需 `c_repet_repeats` 个分离重复；节拍需至少 1 秒历史；稳定度需 `c_centroid_window_frames`。若可用背景证据少于 3 项，令 `C_conf=0` 并保持 UNKNOWN，不做强压。置信度一致性也只在已 ready 的非 B 特征上计算：

```text
u = [M_hpss,R_rep,H_harm,B_beat,N_narrow,S_stable] filtered by ready
var_u = variance(u)
C_agree = 1 - clamp(sqrt(var_u)/c_agreement_scale, 0, 1)
C_conf = C_agree * (0.55 + 0.45*abs(2*C_music_raw-1))
```

若可用证据数少于 3，按上方规则直接令 `C_conf=0`。

再用突发和语音证据削弱“这是 BGM 的纯背景”判断：

```text
C_music_raw = clamp(
    C_bg * (1 - c_event_guard_pct/100*E_event)
         * (1 - (c_voice_guard_pct/100)*V_voice),
    0, 1)
```

这里 B 只占 0.20；它是先验，不是裁决者。C 相对 B 的提升来自 REPET/HPSS/谐波/节拍/新颖度的组合，而不是把 B 的低频分数换个名字。

特征不一致时降低置信度。对背景证据向量：

```text
u = [M_hpss,R_rep,H_harm,B_beat,N_narrow,S_stable]
var_u = variance(u)
C_agree = 1 - clamp(sqrt(var_u)/0.35, 0, 1)
C_conf = clamp(
    C_agree * (0.55 + 0.45*abs(2*C_music_raw-1)),
    0, 1)
```

乘法而不是相加很重要：特征互相矛盾时 `C_agree` 接近 0，置信度也必须接近 0；即使 `C_music_raw` 偏高，也不能强压。这正是“多算法不一致时的保守策略”。

#### 3.4.4.3 滞回状态和快速重收敛

状态只有三种：`NON_MUSIC`、`UNKNOWN`、`MUSIC_LOCK`。进入和退出条件：

```text
进入 MUSIC_LOCK：
  C_music_raw >= c_music_enter       (默认 0.72)
  C_conf       >= c_music_conf_min   (默认 0.60)
  且持续 >= max(c_music_enter_hold_ms,
                 c_beat_lock_updates*c_beat_update_ms)
  且 R_rep >= c_repet_similarity_on，或 B_beat >= c_beat_corr_on
  且重复/节拍候选 lag 连续 c_beat_lock_updates 次稳定

退出 MUSIC_LOCK：
  C_music_raw <= c_music_exit        (默认 0.48)
      且持续 >= c_music_exit_hold_ms (默认 80 ms)
  或 C_conf < c_music_conf_drop      (默认 0.35) 持续 2 帧
  或 N_novel >= c_novelty_unlock     (默认 0.55) 持续 2 帧
  或 total_rms < 55                  持续 >= c_quiet_reset_ms (默认 120 ms)
```

`55` 沿用当前 `mapping::classify_content()` 的 `MODE_QUIET` 硬阈值，避免 C 自己定义第二个安静量纲。进入门槛高、退出快，原因是新技能/受击音不能被旧 BGM 锁住。第一次播放歌曲的 BGM 锁定可能需要约 0.6-1.2 秒，取决于周期；这只影响持续背景抑制，不延迟当前技能事件。

换曲重收敛：

1. `N_novel >= c_novelty_unlock` 连续 2 帧：立即清除 `R_rep` 的旧最优 lag 和 `B_beat` 锁定计数。
2. 进入 `UNKNOWN`，清除旧 `MUSIC_LOCK`；旧持续门控 `p_locked` 以 40 ms 时间常数指数衰减，快速事件门控立即使用新帧证据，不等待这段衰减。
3. 新历史重新累积，不复用旧歌的周期模板。
4. 静音超过 `c_quiet_reset_ms`：清空节拍和重复历史，`MUSIC_LOCK` 直接退出；恢复播放后从未知态重新收敛。
5. 低音/高音能量发生大幅切换但 `N_novel` 不足时，仍由 `C_conf` 下降使门控变保守，避免交叉淡入时继续强压。

#### 3.4.4.4 背景/事件双分量输出

对 100% B 强度候选 `B_full_l,r`，C 用事件分数拆成“事件候选”和“持续背景候选”，不修改 B 自己的公式：

```text
e = clamp(E_event / max(c_event_split_threshold,eps), 0, 1)
e = max(e, event_active ? c_event_min_share : 0)
b_evt_l = B_full_l * e
b_evt_r = B_full_r * e
b_sus_l = B_full_l - b_evt_l
b_sus_r = B_full_r - b_evt_r
```

`c_event_min_share` 默认 0.50、范围 0.20-0.80；仅当 C 事件状态机进入 ATTACK/HOLD 时生效，确保确认的短瞬态不会被 C_bg 高分完全归入背景。最终基线 `B_current` 始终是原有引擎按当前 `smart_strength_pct` 实际计算的 B 输出；只有 C 差值使用 `B_full` 候选。
音乐门控量采用“锁定强压 + 未锁定有界轻压”两级策略：

```text
lock = 1 if MUSIC_LOCK else 0
p_locked = lock*C_music_raw*C_conf
p_unknown = (1-lock)*(c_uncertain_sustain_pct/100)
            *C_music_raw*C_conf*(1-E_event)
m_sustain = c_sustain_suppress_pct/100
m_event = c_event_suppress_pct/100

G_sustain = clamp(1-m_sustain*p_locked-p_unknown, 0, 1)
```

`c_event_split_threshold` 默认 0.45，范围 0.20-0.80；它只决定 B 输出如何分解成持续/事件候选，不改变原始 B 电平。

对于规律 BGM 鼓点，事件部分也适度压低；对于技能/受击突发，`escape` 增大：

```text
escape = clamp(
    (c_wideband_escape_pct/100)*wide
  + 0.25*N_novel
  + 0.20*(1-B_beat),
  0, 1)

p_event_gate = p_locked + p_unknown*(1-escape)
G_event = clamp(1-m_event*p_event_gate*(1-escape), 0, 1)
```

因此默认不确定压制会主动减少持续、低新颖度 BGM 的误震，但上限由 `c_uncertain_sustain_pct=25%` 限制；高新颖度/宽带事件的 `p_unknown` 与事件压制会同步下降。若各证据不可用或 `C_conf` 低，两个 gate 都接近 0，不做强压。
C 以**当前实际 B 输出**为基线；C 的差值同时受两个旋钮控制，但 B 输出本身仍由现有 B 代码计算，不重建其曲线：

```text
B_current = 当前引擎按 smart_strength_pct 实际产生的 B 输出
B_full = 同帧特征下 SmartEnhancer strength=100% 的 B 候选
C_full = 基于 B_full 的事件/背景分量应用 C 滤波后的候选
s = clamp(smart_strength_pct,0,100)/100
u_c = clamp(c_upgrade_pct,0,100)/100
C_at_s = B_current + s*(C_full-B_full)
Final = B_current + u_c*(C_at_s-B_current)
```

该式在常见“B 智能层对基础目标线性混合”的情况下等价于总差值乘 `s*u_c`，但保留 `B_current` 作为真实回退基线，不假设其它 B 逻辑也线性：

- `c_upgrade_pct=0` 时 `Final=B_current`，无损回退当前 B。
- `smart_strength_pct=0` 时 `s=0`、B_current=A，且 `C_at_s=B_current`，Final=A。
- 两个强度均为 100% 时使用完整 C 候选。

`SmartEnhancer` proximity/pitch/尾巴特征每帧只更新一次；计算 full-strength 系数复用同一特征快照，不二次推进历史。模式切换不重置采集、FFT 和特征环形缓冲。C 输出包络需与 B 状态隔离；回到 B 时选择保留的 B 包络状态，不能把 C 尾巴带入。

默认 `c_sustain_suppress_pct=90` 时，高置信 BGM 持续层候选约保留 10%；`c_event_suppress_pct=70` 时规律鼓点最多压到约 30%（还会乘置信度和 escape）；明显宽带/新颖技能的事件保留更多。所有值为设计起点，按 B 对照实验标定。

#### 3.4.4.5 心理声学保守后处理

BGM 判定完成后，再做一个仅针对持续层的频带掩蔽权重。以 HPSS 因果中值累积得到的背景功率 `P_bg[b]`、当前帧功率 `P_now[b]` 为输入，统一转 dB（`eps` 为最小功率）：

```text
L_bg[b] = 10*log10(P_bg[b]+eps)
L_now[b] = 10*log10(P_now[b]+eps)
mask_b = clamp((L_bg[b]-L_now[b]+c_psy_margin_db)/c_psy_range_db, 0, 1)
```

只有当 `MUSIC_LOCK` 且 `mask_b > c_psy_mask_on` 连续 `c_psy_confirm_frames` 帧时，才把该临界带的持续背景分量乘以：

```text
G_psy_b = 1 - (c_psy_mask_pct/100)*mask_b
G_sustain_b = G_sustain * G_psy_b
```

不得对 `b_evt` 或已识别瞬态应用 `G_psy_b`。这样心理声学只帮助消除持续 BGM 中被强背景掩蔽的触觉层，不会因技能与 BGM 同频直接删除峰值事件。

### 3.4.5 失效场景与兜底策略

单一算法没有通吃能力，C 必须明示下列边界：

| 音频场景 | 可能的证据冲突 | 推荐行为 |
|---|---|---|
| 纯钢琴 BGM | 谐波强、节拍弱、起音有打击感 | 等待重复/稳定证据；只压持续和声，钢琴起音保留 |
| 电子合成 BGM | 重复和节拍强、窄带稳定 | 快速进入 MUSIC_LOCK，强压持续层和规律鼓点 |
| 管弦/电影式 BGM | 重复性弱、和声和质心稳定 | 允许较慢进入锁；先轻压持续层，不因低置信度强切 |
| 无 BGM、只有技能音 | 通量/宽带/新颖度高，重复低 | 不进入音乐锁，事件直接走 B/ C |
| 连续技能/连招 | 也可能有节奏规律 | 只有在和声稳定、HPSS 背景强、重复段持续时才压；宽带事件逃逸 |
| 角色语音/播报 | 中频谐波和 AM 强 | `V_voice` 抑制 BGM 分数；交给 B 人声策略，不误当 BGM |
| BGM 鼓点与技能同拍 | 周期证据强，事件也强 | 持续层强压，规律鼓点只保留低比例；宽带技能用 `escape` 保留 |
| Ogg/MP3/AAC 解码伪迹 | 高频 warble/pre-echo 造成小通量 | 对数压缩、短时中值和滞回处理；不使用编码伪迹作标签 |
| 换曲/交叉淡入 | 旧歌历史与新歌特征混合 | 新颖度两帧解锁，40 ms 内打开事件门，重新累积背景证据 |
| 静音后恢复 | 旧节拍锁可能残留 | 静音 120 ms 清空节拍/重复状态，从 UNKNOWN 重新开始 |

#### 不确定时到底“宁可漏震还是宁可误震”

对 C 的 BGM 筛选，推荐**非对称策略**，不能用一句全局口号覆盖所有输出：

- 对持续背景层：用户更在意 BGM 误震，故置信度中等时允许对**持续、重复、低新颖度**成分做中等压制；无证据的频带不做硬静音，避免把游戏环境整体切哑。
- 对事件层：一般不确定时做有界压制，但当谱新颖度、HPSS 打击掩膜和逐带通量同时高时，定义为高置信突发，使用 escape 保留大部分输出。这样优先减少 BGM 误震，同时留出明确的技能保护证据。
- 若“持续 BGM”与“同频持续技能”在可观测特征上不可分，纯音频算法无法保证两者都正确；默认优先压持续背景，但提供 `c_uncertain_sustain_pct` 限制最大压制，并通过 C→B 一键回退避免不可接受的漏震。
- 高置信度 BGM（`MUSIC_LOCK + C_conf` 达标）时强压持续和声底；规律鼓点中压；只有新颖、宽带、非周期突发才大幅逃逸。

因此兜底分三档：

```text
UNKNOWN + 低新颖度持续成分 → 按 c_uncertain_sustain_pct 有界压制
高置信 BGM → G_sustain 强压，规律事件按 c_event_suppress_pct 中压
高置信突发技能 → 背景继续压，E_event/escape 高则事件大部分放行
```

理由：DNF 场景中持续 BGM 误震会长时间占据触觉、降低有效技能层次；但把与 BGM 混在一起且无可区分证据的真实事件完全归零也不可接受，因此不确定压制设上限，并把明显突发事件设为优先通道。

### 3.4.6 纯 DSP BGM 参数表

以下是 C v1 的设计默认值，均标记为待验证；没有任何一项表示已经在用户机器实测完成。默认值优先选择能保护技能事件的保守起点。

| 参数 | 默认值 | 范围 | 调参说明 |
|---|---:|---|---|
| `c_bgm_filter_enabled` | true | bool | C 模式招牌开关；A/B 不受影响。关闭后 C 仍可做事件增强，但不压背景。 |
| `c_hpss_time_frames` | 9 | 5-17 帧 | 因果时间中值窗口；大则背景稳定，小则延迟/抗换曲更好。 |
| `c_hpss_freq_groups` | 7 | 5-11 组 | 频率中值窗口；太大容易把窄带技能抹成背景。 |
| `c_hpss_mask_power` | 1.0 | 0.5-2.0 | 谐波/打击掩膜锐度；先用 1，避免硬切。 |
| `c_log_eps` | `1e-6` | `1e-9..1e-3` | 防止静音段取对数；不应用过大值抬高噪声。 |
| `c_noise_track_alpha` | 0.005 | 0.001-0.02 | 每带噪声 EMA 更新率；低速跟踪，不让瞬态抬高底噪。 |
| `c_noise_track_margin` | 80 | 20-200 int16 RMS | 噪声更新候选相对旧底的上限裕量。 |
| `c_repet_history_ms` | 3000 | 2000-4000 ms | REPET-SIM 固定 ring buffer；至少覆盖 `c_repet_repeats*c_repet_lag_max_ms`。 |
| `c_repet_lag_min_ms` | 85 | 50-150 ms | 重复周期下限，覆盖快速电子节奏但避开单帧抖动。 |
| `c_repet_lag_max_ms` | 1000 | 600-1500 ms | 周期上限；过大增加历史和误锁。 |
| `c_repet_sigma` | 0.18 | 0.05-0.40 | 余弦距离转相似度的尺度；小则严格。 |
| `c_repet_repeats` | 3 | 2-6 次 | 每个候选滞后需要的重复相似样本数；不足时 REPET 不可用。 |
| `c_repet_similarity_on` | 0.68 | 0.55-0.85 | REPET 证据解释门槛；组合计算仍使用连续分数。 |
| `c_repet_lag_stability_pct` | 8 | 3-20% | 相邻慢通道更新的滞后周期允许变化比例。 |
| `c_harmonic_f0_min_hz` | 55 | 40-100 Hz | 谐波候选最低基频。 |
| `c_harmonic_f0_max_hz` | 1000 | 600-1500 Hz | 过高会把语音/噪声误判为和声。 |
| `c_harmonic_partials` | 6 | 3-8 | 谐波数量；越多 CPU 和误判机会越高。 |
| `c_harmonic_tolerance_bins` | 1 | 0-2 bins | 允许频率漂移；编码/变调时可略放宽。 |
| `c_centroid_window_frames` | 19 | 9-31 帧 | 谱质心/MAD 稳定度历史，约 200 ms 起步。 |
| `c_centroid_sigma_hz` | 1200 | 300-4000 Hz | 谱质心变化到稳定度的指数尺度。 |
| `c_rep_update_ms` | 100 | 50-200 ms | REPET 最优滞后/相似度的慢通道更新间隔。 |
| `c_music_unlock_fade_ms` | 40 | 0-150 ms | 换曲后持续门控的平滑释放；事件门控立即用新帧证据。 |
| `c_beat_window_ms` | 2000 | 1000-3000 ms | 节拍自相关历史；不用于快速事件。 |
| `c_beat_update_ms` | 100 | 50-200 ms | 节拍慢通道更新周期。 |
| `c_beat_period_min_ms` | 85 | 50-150 ms | 节拍周期下限。 |
| `c_beat_period_max_ms` | 1000 | 600-1500 ms | 节拍周期上限。 |
| `c_beat_corr_on` | 0.55 | 0.40-0.80 | 节拍相关和 REPET 锁定共用的门槛；不改变 BGM 多证据权重。 |
| `c_beat_stability_pct` | 8 | 3-20% | 连续周期候选允许变化比例，用于 `period_stability`。 |
| `c_beat_lock_updates` | 3 | 2-6 次 | 连续稳定慢通道更新次数。 |
| `c_flatness_low/high` | 0.18 / 0.55 | low: 0.05-0.40; high: 0.30-0.85 | 谱平坦度到宽带分数的映射边界；low 必须小于 high。 |
| `c_novelty_alpha` | 0.08 | 0.02-0.20 | 24 组短期背景谱更新率；越小越不容易把新音快速吸收为背景。 |
| `c_novelty_sim_low/high` | 0.82 / 0.98 | low: 0.60-0.95; high: 0.85-1.0 | 余弦相似度映射到新颖度的边界；low 必须小于 high。 |
| `c_music_enter` | 0.72 | 0.60-0.85 | 进入音乐锁的背景置信度。 |
| `c_music_exit` | 0.48 | 0.30-0.60 | 退出音乐锁的置信度；必须低于进入门槛形成滞回。 |
| `c_music_enter_hold_ms` | 600 | 300-1500 ms | 音乐锁建立等待；避免一记技能误锁。 |
| `c_music_exit_hold_ms` | 80 | 40-300 ms | 退出更快，保护新事件。 |
| `c_music_conf_min` | 0.60 | 0.45-0.80 | 进入锁的最低综合置信度。 |
| `c_music_conf_drop` | 0.35 | 0.20-0.50 | 低于该值连续两帧立即退出。 |
| `c_agreement_scale` | 0.35 | 0.20-0.60 | 证据标准差映射到 `C_agree` 的尺度参数。 |
| `c_novelty_unlock` | 0.55 | 0.35-0.80 | 换曲/突发变化解锁阈值。 |
| `c_quiet_reset_ms` | 120 | 50-500 ms | 静音多久清空节奏和重复历史。 |
| `c_sustain_suppress_pct` | 90 | 0-100% | 高置信 BGM 连续层最大压制。 |
| `c_event_split_threshold` | 0.45 | 0.20-0.80 | 将 B 输出拆成事件/持续候选的阈值；只影响 C 分路，不改 B。 |
| `c_event_min_share` | 0.50 | 0.20-0.80 | C onset/hold 活跃时，保证至少此比例的 B 输出归为事件分量。 |
| `c_event_suppress_pct` | 70 | 0-100% | 高置信规律鼓点事件压制；宽带 escape 会抵消。 |
| `c_uncertain_sustain_pct` | 25 | 0-60% | 未锁定时对持续、低新颖度背景的压制上限；由公式按 `C_conf` 和 `(1-E_event)` 缩放。 |
| `c_psy_mask_pct` | 35 | 0-60% | 心理声学后处理最大额外压制，必须低于持续层主压制。 |
| `c_psy_margin_db` | 6 | 0-18 dB | 目标频带需要高于本底多少 dB 才认为有显著遮蔽。 |
| `c_psy_range_db` | 18 | 6-36 dB | dB 掩蔽强度归一化范围。 |
| `c_psy_mask_on` | 0.60 | 0.40-0.85 | 启用频带掩蔽的阈值。 |
| `c_psy_confirm_frames` | 3 | 1-6 帧 | 连续满足掩蔽条件后才压持续层，避免单帧误判。 |
| `c_event_guard_pct` | 55 | 20-80% | 高频事件证据对 BGM 背景置信度的削弱程度。 |
| `c_voice_guard_pct` | 35 | 0-70% | 语音证据对 BGM 置信度的削弱程度。 |

CPU 目标（未实测）：全部 C 特征、状态更新、门控和事件增强平均增量 ≤2% 单逻辑核（约 20 ms CPU/秒音频），单次 10 ms tick 的新增计算 P99 <2 ms；音频分析线程总平均 <5% 单逻辑核。必须在 PR0 基线框架的目标机器 release/iter 构建中测量，分别报告 HPSS/REPET/谐波/节拍/掩蔽消融耗时。若超预算，按顺序减少谱组数、REPET 候选 lag 数、节拍更新频率、关闭心理声学辅助层；不能关闭快速瞬态通路、C→B→A 旁路或输出安全闸。

---

## 4. 推荐方案：B 下一代事件增强的公式级设计

### 4.1 总体信号处理链

```text
WASAPI loopback（现有）
  → 真实格式解码、mono 混音、L/R RMS（现有）
  → 1024 Hann FFT / 512 hop（现有）
      ├─ A 核心：RMS、内容三分类、幂律映射、攻放/死区
      ├─ B 核心：A + SmartEnhancer（距离/亮度/尾巴/AM → gain/attack/release/bias/click）
      └─ C 候选：同一 FFT 复用，不做第二次变换
           → 五带包络 + 24 组因果 HPSS/中值统计
           → REPET-SIM、谐波、谱稳定、新颖度、节拍周期证据
           → C_music_raw/C_conf + UNKNOWN/NON_MUSIC/MUSIC_LOCK 状态机
           → 背景/事件分量门控 + 有界瞬态事件增强
           → C 候选与 B 目标按 c_upgrade_pct 插值
  → 现有攻放平滑（保持 A/B 参数语义）
  → AudioHapState.out_l/out_r（现有）
  → finish_tick 的 L/R pan 仅应用一次
  → vibration_thread 合流、占空比保护、XInput（现有）
```

关键原则：

1. C 快速事件路径不能等待 REPET/节拍历史；背景锁只影响慢速持续层门控。
2. B 的 `SmartEnhancer` 每帧仍然运行并保持热状态，因此 C 可以瞬时回退 B。
3. 只有 `c_upgrade_pct` 控制 B→C 差值；`smart_strength_pct` 仍只控制 B 层相对 A 的强度。
4. `smart_strength_pct=0` 时 `Final_target` 必须逐帧退化到 A；`c_upgrade_pct=0` 时逐帧等于当前 B。
5. C 复用既有最终 L/R pan，不在 DSP 内再乘一次声像系数。
6. 全部新增计算在音频引擎当前线程、预分配固定内存运行；不得等待 GUI、磁盘、网络或推理线程。

### 4.2 C 的输入量与归一化

设 FFT 当前帧的 5 个频带功率为 `P_i[n]`、频带 RMS 为 `E_i_rms[n]`，频带范围为：

```text
i=0: 20-80 Hz
 i=1: 80-250 Hz
 i=2: 250-2000 Hz
 i=3: 2000-8000 Hz
 i=4: 8000 Hz 以上（不超过 Nyquist）
```

直接复用 `dsp.rs` 已计算的 bin 功率；不做第二次 FFT。功率比例和 RMS 分开定义：

```text
P_i[n] = sum_{k in band_i} |X[k,n]|^2
E_i_rms[n] = sqrt(P_i[n]) / RMS_NORM * XSCALE
r_i[n] = P_i[n] / (sum_i P_i[n] + eps)
```

每带噪声底 `noise_i` 用慢速 EMA 近似。为避免单次技能把背景底抬高，噪声更新候选限制在旧底附近：

```text
x_noise_i = min(E_i_rms, noise_i + c_noise_track_margin)
noise_i[n] = max(c_noise_floor,
    (1-c_noise_track_alpha)*noise_i[n-1]
    + c_noise_track_alpha*x_noise_i)
```

`c_noise_track_margin` 默认 80 int16 RMS，`c_noise_track_alpha=0.005`，`c_noise_floor=1`。这是逐带固定内存 EMA 近似，不宣称是统计分位数。内部对数特征为 `x_i=ln(1+E_i_rms/max(noise_i,1))`。C 的安静硬闸仍使用现有 `total_rms < 55`。

内部事件包络使用对数压缩，减少大音量把所有事件压到满格。`E_i16` 与 `noise_i` 均为 int16 RMS，`eps=1`：

```text
x_i[n] = ln(1 + E_i16[n] / max(noise_i[n], 1))
```


### 4.3 逐带瞬态和谱通量

每带慢基线：

```text
b_i[n] = (1-alpha_b)*b_i[n-1] + alpha_b*x_i[n]
```

默认 `alpha_b=0.035`，和现有总能量慢均线同量级；固定作为 C v1 内部常量，不暴露 UI。上升与下降可使用不同系数，但首版先保持单一系数，减少参数耦合。

归一化分母下限 `h_floor=0.05`（对数压缩后的无量纲值），防止接近零背景时余量长期饱和；低于该值仍由 `c_onset_threshold` 和门槛/安静闸决定是否起振。

每带超基线余量：

```text
h_i[n] = clamp((x_i[n] - b_i[n]) / max(b_i[n], h_floor), 0, 1)
```

逐带谱通量：

```text
f_i[n] = max(0, x_i[n] - x_i[n-1])
F[n]   = sum_i w_i*f_i[n]
F_norm = clamp(F[n] / (sum_i w_i*x_i[n] + eps), 0, 1)
```

默认 `w=[1.15, 1.20, 1.00, 1.10, 0.90]`，不是为了声称某种语义，而是避免极高频噪声单独触发过强事件；需要用 DNF 素材重新标定，标定前属于待验证默认值。

逐带事件方向权重：

```text
W_low  = clamp(r_0 + r_1, 0, 1)
W_mid  = clamp(r_2, 0, 1)
W_high = clamp(r_3 + r_4, 0, 1)
```

### 4.4 谱平坦度与宽带事件

对同一 FFT 的功率 bin `P_k` 计算有效频段的谱平坦度：

```text
SF = exp(mean_k(ln(P_k + eps))) / (mean_k(P_k + eps) + eps)
```

`SF` 越接近 1，频谱越宽/越噪；越接近 0，越像窄带纯音。C 不把 `SF` 当成“爆炸分类器”，只把它作为事件逃逸和音乐门控的一个弱证据：

```text
wide = clamp(
    (SF - c_flatness_low)
    / max(c_flatness_high - c_flatness_low, eps),
    0, 1)
```

这里 `c_flatness_low=0.18`、`c_flatness_high=0.55` 是设计默认值，和 §3.4.4.1 的定义一致，最终应按同一组离线素材统一标定。

这样可以避免仅因为 BGM 含一个低音频率就把所有输出压掉，同时也避免仅凭宽带就断言“这是技能音”。

### 4.5 快速事件分数和阈值滞回

组合事件分数：

```text
band_contrast = sum_i r_i*h_i[n]
T[n] = clamp(
    0.45*F_norm[n]
  + 0.35*band_contrast
  + 0.20*wide[n],
  0, 1
)
```

权重和为 1。每一帧更新 `T[n]`，不使用未来帧，因此不会额外引入 look-ahead。

状态机阈值：

```text
onset 条件：T >= theta_on
release 条件：T <= theta_off
theta_off = theta_on * 0.65
```

默认：

- `theta_on=0.58`。
- `theta_off=0.377`。
- `refractory_ms=65`：一次事件起振后，在该时间内不创建第二个 onset。
- `merge_ms=35`：两个相邻峰若间隔小于该值，合并到同一事件并更新峰值。
- `hold_timeout_ms=300`：超过该时间没有超过 `theta_off` 的帧，事件进入 release。

状态定义：

```text
IDLE → ATTACK：T >= theta_on 且不在 refractory
ATTACK → HOLD：连续 2 帧 T >= theta_off
HOLD → ATTACK：T 再次超过 theta_on，更新峰值但不重复起振
HOLD → RELEASE：T <= theta_off
RELEASE → IDLE：包络低于 0.05 或超时
```

首版可把 `ATTACK` 的连续帧要求设为 1，以满足极短冲击；默认值建议 1，若 BGM 误触发再通过 `merge/refractory` 调整，而不是人为增加固定延迟。

### 4.6 节奏锁和 BGM 抑制的实现引用

本节只保留实现落点，完整的候选算法、特征公式、多证据置信度、滞回、重收敛、背景/事件双分量和失效兜底见 **§3.4 C 模式招牌能力：纯算法 BGM 筛选**。实现时必须以 §3.4 的 `C_music_raw`、`C_conf`、`MUSIC_LOCK`、`G_sustain`、`G_event` 为唯一口径，不得再恢复旧的单一 `P_music` 公式。

落地顺序：

1. 低频包络 `z[n]` 与正差分 `z_d[n]` 写入固定历史环形缓冲。
2. 慢通道每 `c_beat_update_ms` 搜索 `c_beat_period_min_ms..c_beat_period_max_ms` 的归一化自相关。
3. 只有周期稳定、HPSS/REPET/谐波/稳定证据达到组合阈值时，才更新 `MUSIC_LOCK`。
4. 快速事件使用 `E_event` 和 `escape`，不得等待 `MUSIC_LOCK`；锁定只改变持续层和规律事件的门控。
5. 换曲新颖度或静音重置时清空节拍/重复历史，并将门控回到 §3.4 的保守 UNKNOWN 状态。

### 4.7 B 近远感在 C 中的保留和事件化

B 的 `proximity` 继续作为 C 的主距离状态，不改四项特征和权重。C 只做一个事件级派生：

```text
p_event = clamp(
    0.60*proximity
  + 0.25*clamp(T,0,1)
  + 0.15*(1-tail),
  0, 1
)
```

其中 `tail` 是 B 已有的尾巴代理。这样：

- 近场且突然的声音，事件峰值更高、包络更短。
- 远场或拖尾声音，事件峰值更低、收尾更长。
- B 的主状态仍决定基础输出，不会被一个单帧通量完全覆盖。

### 4.8 事件分类和马达路由

C 的事件分类是声学路由分类，不是语义标签：

```text
LOW_IMPACT  = W_low  >= 0.55
HIGH_IMPACT = W_high >= 0.55
WIDE_IMPACT = wide    >= 0.55
MIXED       = 其他
```

双马达分频时，C 事件层按频带权重路由：

```text
route_l = clamp(0.72*W_low  + 0.28*W_mid + 0.18*W_high, 0, 1)
route_r = clamp(0.18*W_low  + 0.28*W_mid + 0.72*W_high, 0, 1)
```

同步模式时：

```text
route_sync = clamp((route_l+route_r)/2, 0, 1)
route_l = route_sync
route_r = route_sync
```

当前 `Analyzer::process()` 尚未获得 `mod.rs` 算出的立体声 pan，且 `finish_tick()` 会在输出端统一乘 `pan_l/pan_r`。因此 C v1 不在 DSP 内额外估算事件左右声像，也不增加第二次 pan 乘法；最终仍由现有 `finish_tick()` 单次应用 pan。这是与现有结构一致的保守实现边界，事件级声像定位列为后续扩展。

### 4.9 事件峰值、时长和输出融合

事件强度：

```text
I_event = clamp(
    (T - theta_on) / max(1-theta_on, eps),
    0, 1
)
I_event *= p_event * g_event
```

事件目标包络的时间常数：

```text
tau_event = clamp(
    c_event_tau_near_ms
    + (c_event_tau_far_ms-c_event_tau_near_ms)*(1-p_event),
    c_event_tau_near_ms, c_event_tau_far_ms
) ms
```

事件状态机在 onset 时把 `event_env` 设为 `I_event`；同一事件仍活跃时取“当前强度”和“指数衰减旧包络”的较大者，静音/换曲解锁时立即清除：

```text
dt = hop / sample_rate
release_event = exp(-dt / tau_event)
event_env[n] = max(I_event[n], event_env[n-1]*release_event)
若状态为 IDLE 或 MUSIC_LOCK 解锁：event_env = 0
```

`dt` 使用真实 hop 时长，不硬编码 10 ms，这样适配不同设备采样率。对满强度 B 候选 `B_full_l/r` 拆分得到 `b_evt`；事件增强只在状态机 ATTACK/HOLD 且 `E_event` 过阈值时生效，并严格乘在 `b_evt` 上：

```text
c_delta_l = event_active*(c_event_gain/100)*event_env*route_l*b_evt_l
c_delta_r = event_active*(c_event_gain/100)*event_env*route_r*b_evt_r

C_full_l = clamp(C_filtered_l + c_delta_l*(1-C_filtered_l), 0, 1)
C_full_r = clamp(C_filtered_r + c_delta_r*(1-C_filtered_r), 0, 1)
```

`c_event_gain` 默认 65%、范围 0-150%。`(1-C_filtered)` 保留输出 headroom；乘 `b_evt` 保证 B/A 基础目标没有事件能量时 C 不凭空造震。`C_full` 是 §3.4.4.4 定义的完整 BGM/事件处理候选；`c_upgrade_pct=0` 时其差值不会进入最终输出。

C 候选目标仍走当前 `Analyzer` 的 `smooth_signal()`，不得增加第二套未平滑的马达写入路径。默认先沿用 A/B 已算出的 attack/release；若 onset 回放证明事件过钝，才允许按下式做有限调制，且需单独通过 B 对照：

```text
attack_c = clamp(attack + 0.15*event_active*event_env, 0.001, 1)
release_c = clamp(release + 0.05*(1-event_env), 0.001, 1)
```

这是非默认候选项；若不改善 P95 onset，就保留现有 attack/release，避免改变 B 已验证的手感。

### 4.10 C 的强度旋钮和参数兼容语义

保留现有 `smart_strength_pct` 的 B 语义，并以 `c_upgrade_pct` 控制 C 相对 B 的增量：

- A：智能层旁路，输出是 `A_target`。
- B：使用现有 `SmartEnhancer`；`smart_strength_pct=0` 逐帧等同 A，100% 为完整 B。
- C：显式 C 模式下先运行当前 B 配置并得到 `B_target`，再计算 BGM/事件候选 `C_candidate`；`smart_strength_pct` 的行为不变，`c_upgrade_pct` 只控制 B→C 差值。

令 `s=smart_strength_pct/100`、`u_c=c_upgrade_pct/100`，最终输出按 §3.4.4.4 的分支插值计算：

```text
B_full = 当前帧的 SmartEnhancer strength=100% 目标
C_full = B_full 经过 C 的 BGM/事件筛选目标
B_current = 当前 B 运行路径实际目标
C_at_strength = B_current + s*(C_full-B_full)
Final = B_current + u_c*(C_at_strength-B_current)
```

实现前要用相同帧输入验证 `C_at_strength` 的定义与 B 强度曲线一致；如发现 B 特征增益与目标幅度不是简单线性关系，应改用对 C_full 的事件/背景增量按 `s` 缩放，但保持 `Final(u_c=0)=B_current` 和 `Final(s=0)=A_current` 两个回退不变量。C 所有差值包括持续 BGM 压制、规律事件压制、心理声学附加压制和事件增强都必须通过 `u_c`。

逐帧出口：

- `smart_strength_pct=0` 时 `s=0`，C 的筛选/增强差值显式置零，Final 精确等于 A_current。
- `u_c=0` 时 Final 精确等于当前 B_current。
- `s=100%` 且 `u_c=100%` 时使用完整 C_full。

旧配置迁移：只有 `audio_algorithm_mode` 缺失时才用既有 `smart_enabled=false/true` 推导 A/B；显式模式字段优先，`"a"/"b"/"c"` 分别选择对应算法。配置没有该字段时绝不默认 C，以保证既有配置输出不变。

### 4.11 参数表：沿用 B 的参数

下表数值来自当前 `config.rs` 默认函数和 `AudioHapticsConfig::default()`；不是 C 新调参结果。C 首版默认沿用，避免切换模式时基础手感突变。

| 参数 | 默认值 | 范围/枚举 | C 中的作用和调参说明 |
|---|---:|---|---|
| `master_gain_pct` | 72 | 0-150% | 总输出。先用它匹配马达舒适度，再调 C 事件层。 |
| `low_gain_pct` | 100 | 0-200% | 低频/L 侧基础量感。低频事件偏弱时调；不建议用它代替 C 路由。 |
| `high_gain_pct` | 100 | 0-200% | 高频/R 侧基础量感。高频技能不足时调。 |
| `punch_pct` | 123 | 0-200 | A/B 幂律对比度。C 不修改；太大可能造成顶部饱和。 |
| `mask_threshold` | 260 | 0-1200 int16 RMS | 基础门槛；C 事件也必须遵守或明确使用“事件余量”规则。 |
| `adaptive_threshold` | true | bool | B/内容层底噪自适应。噪声环境误震时保持开启。 |
| `auto_profile` | false | bool | BGM/人声/安静慢速模式。C 只增加事件门控，不替换它。 |
| `music_mask_pct` | 40 | 0-100% | A/B 现有音乐连续输出压制；C 另用 `MUSIC_LOCK` 和多证据门控，不改变该字段语义。 |
| `quiet_mask` | true | bool | 安静段是否完全静音。C 不应绕过。 |
| `rhythm_pct` | 0 | 0-100% | 现有 BGM 鼓点力度；C 的节奏锁只控制抑制/逃逸，保持此参数语义。 |
| `texture_pct` | 30 | 0-100% | 人声/细节微颤；C 事件脉冲不得污染安静段。 |
| `attack_pct` | 38 | 5-90 | 基础起手速度；C 只允许有限上调。 |
| `release_pct` | 12 | 2-60 | 基础收尾速度；C 事件使用独立包络，但最终仍受此边界影响。 |
| `dead_zone_pct` | 5 | 0-20 | 马达底噪死区。C 事件归零前也应走统一死区。 |
| `map_max` | 4200 | 500-12000 | 基础满量程。影响 B loudness 和 A 曲线，不在 C 内另建满量程。 |
| `dual_motor` | false | bool | true=低/高分频，false=同步；C 严格继承。 |
| `vocal_only` | true | bool | 同步模式下只跟人声频段；C 不绕过。 |
| `bass_high_hz` | 520 | 200-1000 Hz | 低频边界；C 事件低频路由沿用。 |
| `vocal_low_hz` | 650 | 300-1500 Hz | 人声带下限；影响 B/A 内容层。 |
| `vocal_high_hz` | 3600 | 1500-8000 Hz | 人声带上限；影响 B/A 内容层。 |
| `smart_enabled` | false | bool | A/B 兼容开关。C 内部需视为启用 B 核心，但对外保留兼容语义。 |
| `smart_strength_pct` | 70 | 0-100% | B 核心强度；0=A，100=完整 B。C 中语义不改变。 |
| `smart_duration_pct` | 70 | 0-100% | B 近短远长对收尾的影响；C 事件时长在此基础上叠加，不替换。 |

### 4.12 参数表：C 新增参数

以下是建议的内部字段名。具体是否落到配置文件，要在实施 PR 评审时确认；默认 C 模式关闭，旧配置缺字段时使用这些默认值。所有默认均为设计起点，待离线和实机验证。

| 新参数 | 默认值 | 范围 | 调参说明 |
|---|---:|---|---|
| `audio_algorithm_mode` | 旧配置映射 | `"a"/"b"/"c"` | 若字段缺失，`smart_enabled=false→A`、`true→B`；只有用户明确选 C 才启用 C，保证旧配置行为不变。 |
| `c_upgrade_pct` | 100 | 0-100% | 从 B 向完整 C 筛选/事件输出插值；0=B，100=完整 C。 |
| `c_event_gain` | 65 | 0-150% | 事件 headroom 增益。先调它，不要先改总强度。 |
| `c_onset_threshold` | 0.58 | 0.35-0.90 | 事件分数起振阈值。误触发多则提高；漏轻击则降低。 |
| `c_release_ratio` | 0.65 | 0.45-0.85 | `theta_off = theta_on * ratio`。越大越容易保持同一事件。 |
| `c_event_min_share` | 0.50 | 0.20-0.80 | ATTACK/HOLD 时至少保留此比例的 B 电平作为事件分量。 |
| `c_refractory_ms` | 65 | 30-120 ms | 一次事件后禁止新起振的时间。连击被合并时减小；BGM 鼓点误触发时增大。 |
| `c_merge_ms` | 35 | 15-80 ms | 相邻峰合并窗口。避免一个技能起音被拆成多次震动。 |
| `c_hold_timeout_ms` | 300 | 120-600 ms | 没有有效分数后进入 release 的最长等待。 |
| `c_event_tau_near_ms` | 45 | 35-100 ms | 近场事件衰减下限；越小越脆。 |
| `c_event_tau_far_ms` | 200 | 100-300 ms | 远场事件衰减上限；越大拖尾越长。 |
| `c_beat_window_ms` | 2000 | 1000-3000 ms | 节奏历史长度。越长越稳定但适应变速越慢。 |
| `c_beat_corr_on` | 0.55 | 0.40-0.80 | 节拍相关和 REPET 锁定共用门槛；过低会把连击锁成音乐。 |
| `c_beat_period_min_ms` | 85 | 50-150 ms | 节奏周期下限。 |
| `c_beat_period_max_ms` | 1000 | 600-1500 ms | 节奏周期上限。 |
| `c_beat_stability_pct` | 8 | 3-20% | 连续候选周期允许变化；越小越保守。 |
| `c_event_split_threshold` | 0.45 | 0.20-0.80 | 将 B 输出拆成事件/持续候选的阈值；只影响 C 分路，不改 B。 |
| `c_sustain_suppress_pct` | 90 | 0-100% | 高置信 BGM 持续层最大压制；锁定前不强制使用。 |
| `c_event_suppress_pct` | 70 | 0-100% | 高置信规律鼓点事件压制；宽带 escape 会抵消。 |
| `c_uncertain_sustain_pct` | 25 | 0-60% | 未锁定时对持续、低新颖度背景的压制上限；由公式按 `C_conf` 和 `(1-E_event)` 缩放。 |
| `c_wideband_escape_pct` | 70 | 0-100% | 宽带、突发事件从音乐门控中逃逸的权重。 |
| `c_fault_trip_count` | 3 | 1-10 帧 | 连续异常多少帧后旁路 C；避免单帧异常误降级。 |
| `c_warmup_ms` | 340 | 0-1000 ms | 快速 C 特征的基础暖机长度；REPET/节拍有各自更长的 ready 条件。暖机时先使用 B，不延迟 B 起振。 |

内部固定权重建议初值：`F_norm=0.45`、逐带余量 `0.35`、宽带 `0.20`；低频/高频路由主权重 `0.72`；这些不一定暴露为 UI 参数，必须在回放测试中记录实际值和改动理由。

---

## 5. 回退与共存设计

### 5.1 模式关系

```text
用户选择：A / B / C（音频算法互斥）

A：基础 A 映射
B：A + SmartEnhancer
C：A + SmartEnhancer + C 事件层

音频输出与 DLL 输出仍在 vibration_thread 做 max 合流
```

A/B/C 是同一音频源的三种算法，不是三条并发音频线程。C 不另开输出线程、不直接调用 `send_vibration`，避免绕过既有去重、合流和占空比治理。

### 5.2 兼容旧配置

建议新增一个模式字段，例如 `audio_algorithm_mode`，但本轮不改代码。迁移语义应是：

| 配置状态 | 运行模式 |
|---|---|
| 没有 `audio_algorithm_mode` 且 `smart_enabled=false` | A |
| 没有 `audio_algorithm_mode` 且 `smart_enabled=true` | B |
| `audio_algorithm_mode="a"` | A，忽略 B/C 升级层 |
| `audio_algorithm_mode="b"` | B，保留现有 `smart_strength_pct` 语义 |
| `audio_algorithm_mode="c"` | C，先跑 B 核心，再跑 C 事件层 |
| 非法模式字符串 | 记录一次配置错误，降级 B；若 B 参数也非法，再降级 A |

旧 `smart_enabled` 不应立刻删除，否则旧 `Config.toml` 和现有 UI/测试会失去兼容。显式 `audio_algorithm_mode` 优先；C 模式下内部强制开启 B 核心供 C 复用，但写回配置时保留用户原有 `smart_enabled` 值，仅由模式选择作为运行时权威值。C 稳定后再考虑把旧字段变成派生字段。

### 5.3 C → B 的无损回退

必须让 B 核心在 C 运行时持续计算并保持热状态：

1. 当前帧先计算 A 基础目标。
2. 调用现有 `SmartEnhancer` 得到 B 目标和遥测。
3. C 只读取 B 目标，不修改 B 内部历史状态。
4. C 层关闭、未暖机、参数非法或故障时，直接把 B 目标送入原有平滑。

因此 C 回退到 B 时：

- 不需要等待 340 ms 重新暖机 B。
- 不需要重新打开 WASAPI。
- 不改变 B 的 `proximity/pitch/duration`。
- C 的事件历史可以清空，但 B 的输出连续保留。
- 回退日志只写一次，例如“C 事件层异常，已旁路，继续使用 B”。

“无损”验收定义：同一输入、同一 A/B 参数、同一 B 状态下，C 层旁路后的 `level_l/level_r` 与直接 B 模式逐帧误差 ≤1e-6；实机输出则允许受线程时序影响的单 tick 差异，但不得改变平均电平和事件顺序。

### 5.4 B → A 的无损回退

沿用当前已经存在的语义：

- B 模式 `smart_strength_pct=0` 返回中性系数，逐帧等同 A。
- 用户切换到 A 时 `smart_enabled=false`，B 层完全旁路。
- B 输入 NaN/Inf 或状态失效时，`SmartEnhancer` 返回中性输出；C 应继续使用这个中性结果，不得把 B 异常放大。

### 5.5 自动降级策略

C 层每帧做以下检查：

- 所有输入、事件分数、节奏相关量和输出必须 `is_finite()`。
- 环形索引、事件状态和周期范围必须在固定边界内。
- C 融合目标必须在 `[0,1]`，且不能改变 B/A 的基础目标。
- 连续 `c_fault_trip_count` 帧异常才触发自动降级，默认 3 帧。
- C 每次降级后至少保持 1 秒冷却，避免在异常边界反复开关。

降级层级：

```text
C 特征/事件层异常
  → 旁路 C，保持 B
B SmartEnhancer 输入异常或 B 层状态失效
  → 返回中性系数，保持 A
A 基础 DSP/采集异常
  → 现有引擎状态机归零，等待设备重试
```

故障降级不得：

- 直接清空 `AudioHapState.out_l/out_r` 后忘记恢复。
- 让音频线程自行写手柄。
- 把一次短暂输入 NaN 变成整个音频线程退出。
- 把 C 失败误报成 WASAPI 设备故障。

### 5.6 参数和 UI 语义

建议 UI 继续展示 A/B/C 三个并列模式按钮：

- A：经典基线。
- B：智能增强，显示现有“智能增强强度”和“持续时间影响”。
- C：B 下一代，继续显示上述两个 B 参数；额外显示“C 升级强度”（0% 等于 B，默认 100% 开启完整筛选）和“BGM 过滤力度”。

模式优先级：显式 `audio_algorithm_mode` 为唯一权威值；旧配置缺失该字段时，才由既有 `smart_enabled` 映射 A/B。C 是显式单选状态，不得通过 `smart_enabled=true` 隐式默认开启。

切换按钮只改变模式，不自动改总强度、低/高频增益、门槛和预设。C 发生故障时，状态栏显示“C 已降级到 B”，而不是静默切回 A。

---

## 6. 对比实验方案

C 的主对照必须是 B；A 只作附带基线。所有算法用同一份音频、同一采样率、同一马达增益和同一基础参数，避免把音量差异误判为算法提升。

### 6.1 离线素材集

#### 6.1.1 合成素材（有精确标签）

用确定性种子生成，不写入仓库，可放在 `E:\sorahk-build\tmp` 临时目录，评估结束后直接删除：

1. 单次低频冲击：60-200 Hz 指数衰减 burst。
2. 单次高频冲击：2-8 kHz 短 burst。
3. 宽带技能音：低/中/高频同时出现，带短起音和拖尾。
4. 重复鼓点：85、120、250、500 ms 周期，模拟不同 BGM 节奏。
5. 不规则连击：随机间隔 80-500 ms，模拟 DNF 技能序列。
6. 纯音和和声铺底：测试音乐误触发。
7. 语音样 AM：中频带调制、低频较少，测试人声层。
8. 近/远配对：同一波形改变 RMS、亮度、尾巴长度，验证 B 的单调性。
9. 左/右声道配对：同一事件仅改变声道，验证现有最终 pan 只作用一次，避免 C 改造引入左右增益回归。
10. 安静噪声：不同底噪和短暂点击，测试自适应门槛。

合成音频的 onset、持续区间、节拍周期均由生成器写入标签，不依赖人工听标。

#### 6.1.2 真实素材（待采集）

必须采集同一 DNF 客户端、同一输出设备、同一音量下的短片段：

- 单技能命中、连续技能、觉醒/爆发、爆炸/重砸。
- 城镇/副本 BGM，含鼓点和无鼓点片段。
- NPC/角色语音、系统提示音、UI 点击。
- 环境声、引擎/持续低鸣、混合战斗场景。
- 不同游戏音量（30%、60%、100%）和不同背景底噪。

当前仓库未提供统一的标注数据，真实素材、隐私确认和标注流程均为待验证事项。不要把网络音频或用户语音未经许可放入评估集。

### 6.2 标注方式

对真实素材采用“双人时间标注 + 争议复核”：

- `event_onset_ms`：听到/看到技能音开始的时间。
- `event_end_ms`：主要冲击结束，不把长尾全部当成新事件。
- `event_kind`：低频冲击、高频冲击、宽带冲击、持续声、规律节奏、语音、安静。
- `is_repeatable`：是否属于稳定周期节奏。
- `side`：若录音保留有效左右声道，标记左/右/中央；否则记 unknown。

允许两位标注者时间差 ≤30 ms；超出则第三人复核。标签不是“枪声/爆炸”这类超出声学证据的语义标签，除非另有游戏事件同步记录。

### 6.3 离线指标

#### 6.3.1 事件检测

将输出包络中超过 0.10 的局部峰与标签匹配，允许 ±50 ms：

```text
precision = TP/(TP+FP)
recall    = TP/(TP+FN)
F1        = 2*precision*recall/(precision+recall)
```

分别统计：技能事件、规律 BGM、语音、安静段。主判据是 C 相比 B 的 F1 和误触发同时改善，而不是单独追求召回。

#### 6.3.2 BGM 筛选专项指标

BGM 筛选必须单独统计，不能被总事件 F1 掩盖：

- **持续背景抑制率**：在人工标注的 BGM 稳态区间，`mean(B_output)-mean(C_output)` 除以 `mean(B_output)+eps`；按低/中/高频分别报告。
- **技能保留率**：BGM 叠加技能片段中，C 的技能峰值或峰面积除以 B 的对应值；目标不低于 0.90，避免“过滤成功但把技能一起滤掉”。待验证。
- **BGM 误锁率**：无 BGM 的技能/语音片段进入 `MUSIC_LOCK` 的比例；换曲和静音后的旧锁残留时间。
- **锁定延迟**：BGM 开始到 `MUSIC_LOCK` 的时间；目标 0.6-1.2 s，且不会延迟锁定前的事件输出。待验证。
- **解锁延迟**：新颖度/静音发生到 `MUSIC_LOCK` 清除的时间；目标 P95 ≤80 ms。待验证。
- **证据贡献消融**：依次关闭 HPSS、REPET、谐波、节拍和心理声学后处理，报告 BGM 抑制率、技能保留率和 CPU 变化，证明每个模块不是装饰。
- **分场景矩阵**：至少分别报告纯钢琴、电子合成、管弦/电影配乐、无 BGM 技能、连续连招、语音和编码/重采样版本。

#### 6.3.3 时间和颗粒度

- onset 中位误差、P95 绝对误差。
- 相邻标签事件被合并的比例。
- 一个标签事件被拆成多个峰的比例。
- 间隔 ≥80 ms 的事件分离率。
- release 结束误差和长尾持续时间。

#### 6.3.4 手感和路由

- 每类事件的 L/R 峰值、RMS 和峰值出现时间。
- 低频事件 `L/R` 比、高频事件 `R/L` 比。
- 近/远配对的输出比、attack 时间和 release 时间。
- 输出 P25/P50/P95 动态范围。
- 触顶（电平 ≥0.98）占比，比较 C 是否减少 B 的饱和损失。

#### 6.3.5 性能和稳定性

- 音频线程 CPU 平均、P95、P99。
- `process()` 单 tick 墙钟时间平均和 P99。
- 每小时堆分配次数、峰值内存、环形缓冲长度。
- 1 小时连续播放的 NaN/Inf、故障降级次数和占空比保护次数。
- 设备拔插、暂停/恢复、白名单切换下的输出残留。

### 6.4 离线对比矩阵

每个素材至少跑三份结果：

| 版本 | 用途 |
|---|---|
| A | 老手感基线，确认 C 没有退化到更差的基础输出 |
| B | 主对照，验证 C 是否真正升级 B |
| C | 推荐方案，输出事件、节奏、路由和性能指标 |

固定参数建议：先以当前默认值为一组，再以“游戏强劲”预设为一组；C 新参数保持默认，不允许为 C 单独手工调到素材集最优后再比较，否则会产生过拟合。

### 6.5 在线主观 ABX

#### 6.5.1 流程

1. 参与者先熟悉 A、B、C 各 30 秒，不告知当前播放标签。
2. 每个场景制作 A/B 两个版本，音量、总强度、马达增益和输出上限固定。
3. 随机播放 A 或 B 作为已知样本，再随机播放 X，X 等于 A 或 B；参与者选择 X 更接近哪一个，并评价偏好。
4. 主实验以 B vs C 为主；A vs C 只做附带对照。
5. 每个场景至少 3 次重复，随机化顺序，场景间休息，记录疲劳和误震。
6. 不向参与者展示算法名称，只显示样本编号；记录场景、音量、模式、选择和评分。

#### 6.5.2 评分项

每段 1-7 分：

- 起振是否跟手。
- 一次技能是否有“一下”的边界。
- 连续技能是否糊成一团。
- 低频/高频层次是否可分。
- 左右位置是否有帮助。
- BGM/语音是否少乱震。
- 长时间使用是否疲劳。
- 综合偏好。

#### 6.5.3 “C 确实更强”的判定

建议预注册以下出口标准，达到多数才允许称为升级：

- C vs B 的独立事件 F1 提高 ≥10 个百分点，或误触发每分钟下降 ≥30%。
- C 的 P95 起振延迟不比 B 增加超过 3 ms。
- C 的 CPU 平均增量 ≤2 个百分点（约 20 ms CPU/秒音频），且新增 P99 tick 计算 <2 ms。
- C 不破坏 B 的近远单调性、高低路由和强度 0 回退。
- ABX 中 C 被选为更好或更合适的比例达到 ≥70%，并且疲劳评分没有显著恶化。样本量和置信区间需要根据实际参与者数计算，不能只看一次个人体验。

若 C 只提高“更猛”而没有降低误震、颗粒度或延迟，不算“更强更完美”，应回到参数或方案调整。

---

## 7. 实施拆分与验收标准

本轮不实施。下面是建议的最小 PR/步骤拆分；每步通过后再进入下一步。

### PR0：基线录制与回放/基准框架

**范围**

- 建立纯 Rust 的离线帧回放接口，输入预先录制的 mono/L/R 帧或合成信号。
- 固化 A/B 当前输出快照、B 强度 0 等价 A、B 近远单调性测试。
- 增加 `process()` CPU 和分配基线记录。

**验收**

- 同一输入重复运行，A/B 每帧输出确定。
- 现有音频单测全通过；合成测试固定 seed 并可重放。
- 能输出 A/B 的事件包络、L/R 电平和 BGM/事件遥测 CSV/二进制记录。
- 基准以至少 10 秒合成音频循环处理，计时只包住 `Analyzer::process()` 的 C 热路径；报告每秒音频 CPU 毫秒数、P50/P99 tick 和累计 malloc 次数。
- 不改运行时行为；若只添加测试辅助代码，也不得引入新模型或网络。

### PR1：C 状态容器和特征提取（旁路输出）

**范围**

- 在 `Analyzer` 内增加固定大小的逐带历史、逐带基线、事件短历史和 C 状态。
- 复用当前 FFT bin，不做第二次 FFT。
- 实现 `x_i`、`h_i`、`F_norm`、`SF`、`W_low/W_mid/W_high` 和有限值保护。
- 实现 24 个对数谱组的 HPSS/因果中值统计、短期稳定度和谱新颖度；先只输出遥测，不参与马达。
- C 默认旁路，输出必须与 B 完全一致。

**验收**

- 合成低频、高频、宽带、纯音和安静输入的特征方向符合公式。
- 纯钢琴/和声与宽带冲击在 HPSS 背景掩膜和事件掩膜方向上可区分；对单一算法失效场景有回归样例。
- C 旁路与 B 逐帧差 ≤1e-6。
- 固定内存、无新增热路径堆分配；若当前 FFT 工作区仍分配，必须记录并给出后续优化任务。
- 合成信号单测：4 秒 220/330 Hz 和声 + 每 500 ms 低频鼓点；4 秒无节拍钢琴和弦；8 Hz/10 Hz/随机间隔技能 burst；静音 2 秒后恢复；同一 BGM 突变到不同频带/节奏的新曲。逐项断言音乐置信度/锁、事件分数、技能保留、静音归零、换曲解锁时间。
- 性能基准：10 秒固定种子混合音频循环至少 10 次，报告处理 1 秒音频的 CPU 毫秒数；目标 C 新增算法 ≤20 ms CPU/秒音频，且音频线程平均 <5% 单逻辑核。超过任一门槛必须先优化，不能只声明待验证。
- 基准和合成 BGM 筛选输出需同时保存 B/C 对照，证明持续 BGM 下降、突发事件仍通过；以上为实施验收，不是当前实测结果。

### PR2：事件状态机和节奏锁

**范围**

- 加入 `theta_on/theta_off` 滞回、refractory、merge、hold/release。
- 加入低频包络历史和低频率节拍自相关；节奏计算每 100 ms 一次。
- 加入 REPET-SIM 滞后相似度、谐波性、谱稳定度、`C_music_raw/C_conf`、`MUSIC_LOCK` 和 `escape` 遥测。
- 节奏锁只影响门控，不得阻塞快速事件通路。

**验收**

- 单次冲击只能产生一个事件。
- 间隔 ≥80 ms 的两次合成冲击可按目标比例分离。
- 稳定周期电子 BGM 在满足 HPSS/REPET 或节拍证据和其它一致性证据后进入 lock；纯钢琴/管弦、随机技能、语音分别有合成样例，随机技能和语音不得长期 lock。
- 换曲新颖度触发后旧持续门控在 40 ms 目标时间常数下释放；静音超时清空历史，事件通路立即不受旧 lock 影响。
- 事件软件延迟相对 B 增量 ≤3 ms。
- BGM 筛选专项通过率、技能保留率、误锁率按 §6.3.2 统计；宽带突发事件不得因音乐锁整体静音。

### PR3：B 基础上的 C 事件融合和回退

**范围**

- 实现 `p_event`、事件包络、低/高频路由和 headroom 公式；复用 `finish_tick()` 已有单次 L/R pan，不在 DSP 内叠加第二次声像系数。
- 新增 `c_upgrade_pct`、`c_event_gain` 等配置/运行态镜像，或先以内部默认常量完成验证。
- 实现 C→B 自动降级、B→A 中性回退和故障节流日志。

**验收**

- `c_upgrade_pct=0` 与 B 逐帧一致。
- C 故障注入（NaN、非法周期、环形边界、强制异常）后 3 帧内旁路 C，B 继续输出。
- B 故障注入后输出与 A 一致，不触发线程退出。
- C 不改变 `dual_motor=false` 的同步语义，不绕过统一输出出口。
- 离线主指标达到第 2 节最低目标，未达到则不进入 UI/发版。

### PR4：模式配置、UI 和遥测

**范围**

- 增加 A/B/C 模式选择，旧 `smart_enabled` 配置迁移兼容。
- 增加 C 事件层滑条和节奏抑制显示；保留 B 原有强度/持续时间滑条语义。
- 遥测显示当前模式、事件状态、`E_event`、`C_music_raw`、`C_conf`、`MUSIC_LOCK`、`c_upgrade_pct`、降级原因。
- 任何模式切换不改总强度、频带增益和预设字段。

**验收**

- 旧 Config.toml 无 C 字段仍能启动，默认行为不变。
- A/B/C 一键切换后状态、输出和 UI 文案一致。
- C→B 时无需重启采集；B→A 后 `smart_active=false` 且逐帧回退成立。
- 配置异常时显示“已降级到 B/A”，不静默失败。

### PR5：实机调参、长稳和发布门槛

**范围**

- 使用第 6 节素材和 DNF 实机流程调参。
- 完成不同音量、不同设备、设备拔插、暂停/白名单、DLL 合流和占空比保护测试。
- 完成 ABX，记录 B vs C 主结论。

**验收**

- 通过第 2 节延迟、CPU、F1/误触发、动态范围和回退指标。
- 连续 1 小时没有 C 状态泄漏、NaN、输出卡满或自动降级风暴。
- 只有当 C 相比 B 在客观和主观指标上都达到预设门槛，才把 C 标为推荐模式；否则保持实验开关或默认关闭。

---

## 8. 最终建议

推荐实施路线一：**B 智能层 + 因果 HPSS/中值背景估计 + REPET-SIM + 谐波/谱稳定 + 节拍锁 + 多带瞬态状态机 + 频带马达路由**。

它强的原因不是把输出简单调大，而是保留 B 已验证的近远、高低、短长手感，再把当前总能量中混合的“持续背景、重复节奏、和声结构、事件起点和频带归属”拆开判断。它不引入模型权重或 GPU，复用当前 FFT 和 `SmartEnhancer`，并将 C 变化限制为 B→C 差值，形成可测试的 `C → B → A` 回退链。

CPU 预算以音频线程平均低于单逻辑核 5%、C 相对 B 增量不超过 2 个百分点（约 20 ms CPU/秒输入音频）、新增单 tick P99 小于 2 ms 为验收线；这些数字在 PR0 基线和目标机器 benchmark 完成前均属于待验证，不能提前宣称已达标。
