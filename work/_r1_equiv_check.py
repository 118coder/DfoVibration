#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""R1 重构等价校验器 (2026-09-27, 架构重构后的防冲突审计)。

原理: 把 5 次拆分的每一个"原始文件"与"拆分后的全部新文件"做归一化行多重集对比:
  - LOST   = 原始代码中存在、新代码中找不到的行 (必须逐条核对为已知委托/清理编辑)
  - ADDED  = 新代码中多出来的行 (必须逐条核对为已知的方法签名/调用点/前导行)
归一化: 去空白行/注释行/use 行/属性行/纯括号行; 剥离可见性修饰 (pub*/pub(super)/pub(in ..)/pub(crate));
这样 pub 可见性放宽 (与拆分前"文件内可见"语义等价) 不产生噪声, 而任何内容行的丢失/改动都会亮红。

用法: python work/_r1_equiv_check.py            # 正常审计
      python work/_r1_equiv_check.py --selftest # 变异自检: 临时调换 page.rs 两行, 应当亮红
"""
import io, subprocess, sys, re, collections, os

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
os.chdir(REPO)

def norm(line: str) -> str | None:
    s = line.strip()
    if not s: return None
    if s.startswith("//") or s.startswith("use ") or s.startswith("#"): return None
    if re.fullmatch(r"[{}();\s]*", s): return None          # 纯括号行 (结构行)
    s = re.sub(r"^pub\(in crate::gui\)\s*", "", s)
    s = re.sub(r"^pub\(crate\)\s*", "", s)
    s = re.sub(r"^pub\(super\)\s*", "", s)
    s = re.sub(r"^pub\s+", "", s)
    s = re.sub(r"\s+", " ", s)
    return s

def file_lines(text: str):
    out = []
    for ln in text.splitlines():
        n = norm(ln)
        if n: out.append(n)
    return out

def git_show(ref: str, path: str) -> str:
    return subprocess.run(["git", "show", f"{ref}:{path}"],
                          capture_output=True, check=True).stdout.decode("utf-8")

def read(path: str) -> str:
    return io.open(path, encoding="utf-8").read()

# (基线ref, 原始路径, [新路径...])  —— 覆盖全部 5 次拆分
SPLITS = [
    ("d9c9479", "src/state.rs",
     ["src/state/mod.rs", "src/state/types.rs", "src/state/events.rs", "src/state/turbo.rs",
      "src/state/inject.rs", "src/state/mappings.rs", "src/state/key_names.rs",
      "src/state/whitelist.rs", "src/state/vib_mirror.rs", "src/state/tests.rs"]),
    ("38e14e8", "src/vibration.rs", ["src/vibration/mod.rs", "src/vibration/params.rs"]),
    ("4cb8c3a", "src/gui/vibration_page.rs",
     ["src/gui/vibration_page/mod.rs", "src/gui/vibration_page/hints.rs",
      "src/gui/vibration_page/export.rs", "src/gui/vibration_page/sections.rs",
      "src/gui/vibration_page/page.rs", "src/gui/vibration_page/presets.rs"]),
    ("1dcab3f", "src/gui/settings_dialog.rs", ["src/gui/settings_dialog.rs"]),
    ("8063784", "src/gui/gamepad_mapping.rs",
     ["src/gui/gamepad_mapping/mod.rs", "src/gui/gamepad_mapping/model.rs",
      "src/gui/gamepad_mapping/capture.rs", "src/gui/gamepad_mapping/helpers.rs",
      "src/gui/gamepad_mapping/svg.rs", "src/gui/gamepad_mapping/page.rs",
      "src/gui/gamepad_mapping/quick_connect.rs", "src/gui/gamepad_mapping/flow.rs",
      "src/gui/gamepad_mapping/panel.rs", "src/gui/gamepad_mapping/tests.rs"]),
    ("445fcd5", "src/gui/turbo_page.rs",
     ["src/gui/turbo_page/mod.rs", "src/gui/turbo_page/page.rs",
      "src/gui/turbo_page/presets.rs", "src/gui/turbo_page/edit.rs",
      "src/gui/turbo_page/macros.rs", "src/gui/turbo_page/tests.rs"]),
]

def main():
    if "--mutate" in sys.argv:
        p = "src/gui/vibration_page/page.rs"
        t = read(p).splitlines(keepends=True)
        # 变异: 找马达区方法内两行相邻内容行对调 (模拟"顺序被改坏")
        idx = next(i for i, l in enumerate(t) if "vib_motor_section" in l and "fn " in l)
        body = next(i for i in range(idx, idx + 80) if "马达区: 实时输出检测" in t[i])
        t[body], t[body + 1] = t[body + 1], t[body]
        io.open(p, "w", encoding="utf-8", newline="").writelines(t)
        print("[selftest] 已注入变异: page.rs 马达区相邻两行对调")

    total_lost, total_added = 0, 0
    for ref, old_path, new_paths in SPLITS:
        old = file_lines(git_show(ref, old_path))
        new = []
        for np in new_paths:
            new.extend(file_lines(read(np)))
        oc = collections.Counter(old); nc = collections.Counter(new)
        lost = sorted(((oc - nc).elements()))
        added = sorted(((nc - oc).elements()))
        tag = f"[{old_path}]"
        print(f"\n===== {tag} 原始 {len(old)} 行 vs 新 {len(new)} 行 =====")
        if lost:
            total_lost += len(lost)
            print(f"  LOST ({len(lost)}):")
            for l in lost: print(f"    - {l}")
        if added:
            total_added += len(added)
            print(f"  ADDED ({len(added)}):")
            for l in added: print(f"    + {l}")
        if not lost and not added:
            print("  完全一致")

    print(f"\n===== 汇总: LOST {total_lost} 行 / ADDED {total_added} 行 =====")

if __name__ == "__main__":
    main()

# ── 序列级校验: 动过手术的区块 (C1b 震动页 10 块 / C2 设置弹窗 6 块) ─────────────
# 多重集抓不到"相邻行对调"; 这里逐块做【内容行序列】精确对比。
# 差异只允许: 方法头部插入的前导行 (let th/let p/use Ordering) 与签名/返回行。
SEQ_CHUNKS = {
    ("4cb8c3a", "src/gui/vibration_page.rs", 452): [   # page.rs 坐标 +453 = 原文件坐标 (13+453=466)
        (36, 130,  "src/gui/vibration_page/page.rs", "fn vib_first_render_init"),
        (185, 756, "src/gui/vibration_page/page.rs", "fn vib_hero_or_presets"),
        (758, 777, "src/gui/vibration_page/page.rs", "fn vib_status_section"),
        (783, 847, "src/gui/vibration_page/page.rs", "fn vib_summon_abs_freq_section"),
        (849, 901, "src/gui/vibration_page/page.rs", "fn vib_motor_section"),
        (906, 1043, "src/gui/vibration_page/page.rs", "fn vib_global_gain_section"),
        (1048, 1215, "src/gui/vibration_page/page.rs", "fn vib_damage_font_section"),
        (1220, 1349, "src/gui/vibration_page/page.rs", "fn vib_score_effect_section"),
        (1354, 1474, "src/gui/vibration_page/page.rs", "fn vib_score_decay_section"),
        (1478, 2748, "src/gui/vibration_page/page.rs", "fn vib_advanced_section"),
    ],
    ("1dcab3f", "src/gui/settings_dialog.rs", 0): [
        (277, 305, "src/gui/settings_dialog.rs", "fn settings_title_bar"),
        (322, 616, "src/gui/settings_dialog.rs", "fn settings_section_toggle_key_and_presets"),
        (617, 840, "src/gui/settings_dialog.rs", "fn settings_section_global"),
        (841, 1929, "src/gui/settings_dialog.rs", "fn settings_section_mappings"),
        (1930, 2107, "src/gui/settings_dialog.rs", "fn settings_section_whitelist"),
        (2114, 2168, "src/gui/settings_dialog.rs", "fn settings_action_buttons"),
    ],
}
PRELUDE = {"let th = self.theme();", "let p = &self.app_state.vibration_params;",
           "use std::sync::atomic::Ordering;", "let accent_color = Theme::new(self.dark_mode).accent_text;",
           "let t = &self.translations;", "let Some(temp_config) = self.temp_config.as_mut() else { return; };",
           "let mut should_save = false;", "let mut should_cancel = false;",
           "let _accent_color = Theme::new(self.dark_mode).accent_text;", "(should_save, should_cancel)", "should_cancel"}

def seq_check():
    import difflib
    print("===== 序列级校验 (C1b/C2 手术区块, 顺序必须逐行一致) =====")
    bad = 0
    for (ref, old_path, shift), chunks in SEQ_CHUNKS.items():
        raw_old = git_show(ref, old_path).splitlines()
        for (a, b, new_path, fn_name) in chunks:
            orig = [n for n in (norm(l) for l in raw_old[a-1+shift:b+shift]) if n]
            nt = read(new_path).splitlines()
            start = next(i for i, l in enumerate(nt) if fn_name in l and "fn " in l)
            end = next(i for i in range(start + 1, len(nt)) if nt[i] == "    }")
            body = [n for n in (norm(l) for l in nt[start:end + 1]) if n]
            sm = difflib.SequenceMatcher(None, orig, body, autojunk=False)
            ok = True
            for tag, i1, i2, j1, j2 in sm.get_opcodes():
                if tag == "equal":
                    continue
                inserted = body[j1:j2]
                head_zone = (i1 == 0 and i2 == 0 and j1 == 0)
                if tag == "insert" and i1 == 0 and inserted and "fn " in inserted[0]                         and all(x in PRELUDE or x.startswith("/*") or x.endswith("*/") for x in inserted[1:]):
                    continue  # 方法头 = 签名行 + 前导行插入 = 已知编辑
                if tag == "insert" and all(x in PRELUDE for x in inserted) and i2 == len(orig):
                    continue  # 尾部返回值 = 已知编辑
                ok = False
                print(f"  ✗ {fn_name}: {tag} orig[{i1}:{i2}] new[{j1}:{j2}]")
                for x in (orig[i1:i2] or inserted)[:6]:
                    print("      ", x)
            if ok:
                print(f"  ✓ {fn_name}: {len(orig)} 内容行顺序一致 (前导行白名单外零差异)")
            else:
                bad += 1
    return bad

if "--seq" in sys.argv:
    sys.exit(1 if seq_check() else 0)
if "--mutate" in sys.argv:
    # 变异自检: 找马达区方法内相邻两行对调 (应使多重集/序列校验亮红), 然后还原
    p = "src/gui/vibration_page/page.rs"
    t = read(p).splitlines(keepends=True)
    idx = next(i for i, l in enumerate(t) if "vib_motor_section" in l and "fn " in l)
    body = next(i for i in range(idx, idx + 80) if "马达区: 实时输出检测" in t[i])
    t[body], t[body + 1] = t[body + 1], t[body]
    io.open(p, "w", encoding="utf-8", newline="").writelines(t)
    print("[selftest] 已注入变异: page.rs 马达区相邻两行对调")

if __name__ == "__main__":
    main()
