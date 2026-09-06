/* ============================================================
 * DFO 震动插件 - x86 inline hook 引擎
 * 利用 DFO.exe 主段 R/W/X 权限直接改写函数前 5 字节
 * ============================================================ */
#include <windows.h>
#include <math.h>
#include "hooks.h"
#include "../common/vib_protocol.h"

/* 汇编 stub 符号(C 侧无下划线, 汇编侧带) */
extern void vib_stub_0(void);
extern void vib_stub_1(void);
extern void vib_stub_2(void);
extern void vib_stub_3(void);
extern void vib_stub_4(void);
extern void vib_stub_5(void);
extern void vib_stub_6(void);
extern void vib_stub_7(void);
extern void vib_stub_8(void);
extern void vib_stub_9(void);
extern void vib_stub_10(void);
extern void vib_stub_11(void);
extern void vib_stub_12(void);

void *vib_tramp_0, *vib_tramp_1, *vib_tramp_2, *vib_tramp_3,
     *vib_tramp_4, *vib_tramp_5, *vib_tramp_6, *vib_tramp_7,
     *vib_tramp_8, *vib_tramp_9, *vib_tramp_10, *vib_tramp_11,
     *vib_tramp_12;

/* 采集输出回调(由 dfovib.c 提供): type, strength, tick */
extern void vib_collect(int type, DWORD strength, DWORD tick, DWORD count);
extern volatile int g_ringReady;

/* ---------------- 事件采集 ----------------
 * 在游戏线程上下文执行, 必须极短且不阻塞。
 * OnDamage: a3 = 伤害对象, vtable+808 取伤害值对象, +68 = 伤害数值
 * OnResetComboDamage: a2 = 剩余伤害百分比(可能 >100)
 * 其余事件强度按类型给默认值, EXE 侧再按增益调制。
 * 指针全部来自游戏自身调用栈(合法), 直接读取, 不做异常处理。 */
typedef DWORD (__attribute__((thiscall)) *FnGetDamageObj)(DWORD self);

void __cdecl vib_dispatch_common(void *obj, DWORD a2, DWORD a3, int type)
{
    DWORD str = 0;
    switch (type) {
    case VEV_ATTACK:
        str = 100;
        break;
    case VEV_DAMAGE: {
        /* a3 为伤害对象; 尝试读取伤害数值, 失败则用 100 占位 */
        DWORD *vt;
        DWORD valObj;
        if (!a3) { str = 100; break; }
        vt = *(DWORD **)a3;
        if (!vt) { str = 100; break; }
        valObj = ((FnGetDamageObj)vt[DAMAGE_OBJ_VT_OFF / 4])(a3);
        if (valObj) {
            str = *(DWORD *)(valObj + DAMAGE_VALUE_OFF);
            if (str > 100000) str = 100000;
        } else str = 100;
        break;
    }
    case VEV_RESET_COMBO:
        str = (a2 > 10000) ? 10000 : a2;
        break;
    case VEV_TARGET_DIE:
    case VEV_START_BATTLE:
    case VEV_SHAKE_INPUT:
        str = 1000;
        break;
    default:
        str = 100;
        break;
    }
    vib_collect(type, str, GetTickCount(), 0);
}

/* 伤害飘字 hook 专用分发: this = 飘字对象, 字段 +4/+5/+6/+8 为参数拷贝
 * 普通伤害/暴击/破招/评分文字 的字段模式不同, 用于分类评分事件 */
extern void dll_log2(const char *fmt, ...);
static volatile DWORD s_lastHitLog = 0;

static int ptr_valid(DWORD addr)
{
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery((LPCVOID)addr, &mbi, sizeof(mbi)) == 0)
        return 0;
    return mbi.State == MEM_COMMIT;
}

/* 英文关键词 -> 评分事件(幅度见用户方案表)
 * 返回: 事件类型; *strength 输出幅度(0-100) */
static int classify_font(const wchar_t *txt, DWORD *strength)
{
    if (!txt || !txt[0]) return VEV_DAMAGE;
    /* 纯数字 = 普通伤害飘字 */
    {
        const wchar_t *p = txt;
        int allDigits = 1, digits = 0;
        while (*p && *p < 0x80) {
            if (*p >= '0' && *p <= '9') { digits++; p++; continue; }
            if (*p == ',' || *p == '.' || *p == ' ') { p++; continue; }
            allDigits = 0;
            break;
        }
        if (allDigits && digits > 0) return VEV_DAMAGE;
    }
    /* 评分等级 */
    if (wcsstr(txt, L"SSS") || wcsstr(txt, L"EXCELLENT")) { *strength = 100; return VEV_RATING; }
    if (wcsstr(txt, L"SS ") || wcsstr(txt, L"SS") || wcsstr(txt, L"GREAT")) { *strength = 80; return VEV_RATING; }
    if (wcsstr(txt, L"S ") || wcsstr(txt, L"GOOD")) { *strength = 60; return VEV_RATING; }
    if (wcsstr(txt, L"CRITICAL") || wcsstr(txt, L"CRIT")) { *strength = 80; return VEV_RATING; }
    /* 背击/破招 (2026-08-15): 独立事件 VEV_BACK(16)/VEV_BREAK(15)
     * 触发条件(用户提供): 背击=攻击者与目标朝向相同; 破招=受击对象在攻击/被控制状态 */
    if (wcsstr(txt, L"破招") || wcsstr(txt, L"BREAK") || wcsstr(txt, L"COUNTER")) { *strength = 80; return VEV_BREAK; }
    if (wcsstr(txt, L"背击") || wcsstr(txt, L"BACK") || wcsstr(txt, L"REAR")) { *strength = 40; return VEV_BACK; }
    if (wcsstr(txt, L"GUARD")) { *strength = 50; return VEV_RATING; }
    if (wcsstr(txt, L"AIR")) { *strength = 20; return VEV_RATING; }
    if (wcsstr(txt, L"FINAL")) { *strength = 40; return VEV_RATING; }
    if (wcsstr(txt, L"COMBO")) { *strength = 10; return VEV_COMBO; }
    if (wcsstr(txt, L"PERFECT")) { *strength = 70; return VEV_RATING; }
    if (wcsstr(txt, L"ARMOR") || wcsstr(txt, L"INVINCIBLE")) { *strength = 30; return VEV_RATING; }
    if (wcsstr(txt, L"DODGE") || wcsstr(txt, L"EVADE")) { *strength = 30; return VEV_RATING; }
    /* 未识别: 按普通飘字 */
    return VEV_DAMAGE;
}

/* 供 dfovib.c 的 TextOutW hook 复用 */
int classify_font_text(const wchar_t *txt, DWORD *strength)
{
    return classify_font(txt, strength);
}

/* ---------------- 事件采集(聚合计数版) ----------------
 * hook 回调在游戏线程执行, 只做原子计数(纳秒级), 零内存操作
 * 专职聚合线程每 12ms 批量发送, 彻底消除高频卡顿 */

/* 每类事件计数器(按 a6 主分类) */
static volatile LONG g_cnt_hp = 0;      /* 0x20 DOT */
static volatile LONG g_cnt_special = 0; /* 0x10 特殊攻击 */
static volatile LONG g_cnt_state = 0;   /* 0x08 状态 */
static volatile LONG g_cnt_effect = 0;  /* 0x04 装备特效 */
static volatile LONG g_cnt_attack = 0;  /* 0x01 命中 */
static volatile LONG g_cnt_hit = 0;     /* 0x02 受击 */

void __cdecl vib_dispatch_common_hit(void *obj, DWORD a2, DWORD a3, DWORD a6, int type)
{
    (void)obj; (void)a2; (void)a3; (void)type;
    /* 按主分类计数(优先级与引擎一致: 0x20 > 0x10 > 0x08 > 0x04 > 0x01 > 0x02) */
    if (a6 & 0x20)      InterlockedIncrement(&g_cnt_hp);
    else if (a6 & 0x10) InterlockedIncrement(&g_cnt_special);
    else if (a6 & 0x08) InterlockedIncrement(&g_cnt_state);
    else if (a6 & 0x04) InterlockedIncrement(&g_cnt_effect);
    else if (a6 & 0x01) InterlockedIncrement(&g_cnt_attack);
    else if (a6 & 0x02) InterlockedIncrement(&g_cnt_hit);
#ifdef VIB_DEBUG_TXT
    /* 调试: 逐条飘字事件 (限频 200ms, 观察评分时的事件流) */
    {
        static volatile LONG s_fontIn = 0;
        static DWORD s_last = 0;
        DWORD now = GetTickCount();
        if (InterlockedCompareExchange(&s_fontIn, 1, 0) == 0) {
            if (now - s_last >= 200) {
                s_last = now;
                dll_log2("FONT a6=0x%08X type=%d", a6, type);
            }
            InterlockedExchange(&s_fontIn, 0);
        }
    }
#endif
}

/* 聚合线程调用: 取各计数器增量并发送(由 dfovib.c 的 collector 线程执行) */
void vib_flush_counts(void)
{
    LONG v;
    v = InterlockedExchange(&g_cnt_hp, 0);
    if (v > 0) vib_collect(VEV_FONT, 0x20, GetTickCount(), (DWORD)v);
    v = InterlockedExchange(&g_cnt_special, 0);
    if (v > 0) vib_collect(VEV_FONT, 0x10, GetTickCount(), (DWORD)v);
    v = InterlockedExchange(&g_cnt_state, 0);
    if (v > 0) vib_collect(VEV_FONT, 0x08, GetTickCount(), (DWORD)v);
    v = InterlockedExchange(&g_cnt_effect, 0);
    if (v > 0) vib_collect(VEV_FONT, 0x04, GetTickCount(), (DWORD)v);
    v = InterlockedExchange(&g_cnt_attack, 0);
    if (v > 0) vib_collect(VEV_FONT, 0x01, GetTickCount(), (DWORD)v);
    v = InterlockedExchange(&g_cnt_hit, 0);
    if (v > 0) vib_collect(VEV_FONT, 0x02, GetTickCount(), (DWORD)v);
}


/* ---------------- hook 安装 ---------------- */

typedef struct {
    DWORD target;      /* 被 hook 函数地址 */
    DWORD patchRel;    /* patch 时目标到 stub 的距离(缓存) */
    BYTE  original[16];/* 原始字节 */
    int   hookLen;     /* 覆盖长度(指令边界对齐, >=5) */
    void *stub;        /* 对应 stub */
    void **trampVar;   /* 对应 trampoline 全局变量 */
    int   installed;
} HookCtx;

static HookCtx g_hooks[VIB_MAX_HOOKS];
static int g_hookCount = 0;
static int g_installed = 0;

static void *_vib_stubs[VIB_MAX_HOOKS] = {
    (void*)vib_stub_0, (void*)vib_stub_1, (void*)vib_stub_2,
    (void*)vib_stub_3, (void*)vib_stub_4, (void*)vib_stub_5,
    (void*)vib_stub_6, (void*)vib_stub_7, (void*)vib_stub_8,
    (void*)vib_stub_9, (void*)vib_stub_10, (void*)vib_stub_11,
    (void*)vib_stub_12
};

static void **const g_trampVars[VIB_MAX_HOOKS] = {
    &vib_tramp_0, &vib_tramp_1, &vib_tramp_2, &vib_tramp_3,
    &vib_tramp_4, &vib_tramp_5, &vib_tramp_6, &vib_tramp_7,
    &vib_tramp_8, &vib_tramp_9, &vib_tramp_10, &vib_tramp_11,
    &vib_tramp_12
};

static BYTE *g_trampPage = NULL;

/* 精简 x86 指令长度反汇编器: 返回一条指令长度 (函数头常见指令即可) */
static int insn_len(BYTE *p)
{
    BYTE op = p[0];
    switch (op) {
    case 0x90: case 0xC3: case 0xCC: case 0xF4:
    case 0xF5: case 0xF8: case 0xF9: case 0xFA: case 0xFB:
        return 1;
    case 0xC2: return 3;                    /* retn imm16 */
    case 0x50: case 0x51: case 0x52: case 0x53: case 0x54:
    case 0x55: case 0x56: case 0x57:         /* push reg */
    case 0x58: case 0x59: case 0x5A: case 0x5B: case 0x5C:
    case 0x5D: case 0x5E: case 0x5F:         /* pop reg */
    case 0x40: case 0x41: case 0x42: case 0x43: case 0x44:
    case 0x45: case 0x46: case 0x47: case 0x48: case 0x49:
    case 0x4A: case 0x4B: case 0x4C: case 0x4D: case 0x4E: case 0x4F:
        return 1;
    case 0x6A: return 2;                    /* push imm8 */
    case 0x68: return 5;                    /* push imm32 */
    case 0xE9: case 0xE8: return 5;         /* jmp/call rel32 */
    case 0xEB: return 2;                    /* jmp rel8 */
    case 0x0F:                              /* 两字节 opcode */
        return (p[1] & 0xF0) == 0x80 ? 6 : 2; /* 0F 8x jcc rel32=6, 其余按2(近似) */
    case 0x66: case 0x67: case 0xF3: case 0xF2: case 0x2E:
    case 0x36: case 0x3E: case 0x26: case 0x64: case 0x65:
        return 1 + insn_len(p + 1);         /* 前缀 + 后续 */
    case 0xB0: case 0xB1: case 0xB2: case 0xB3: case 0xB4:
    case 0xB5: case 0xB6: case 0xB7: return 2;   /* mov reg8, imm8 */
    case 0xB8: case 0xB9: case 0xBA: case 0xBB: case 0xBC:
    case 0xBD: case 0xBE: case 0xBF: return 5;   /* mov reg, imm32 */
    case 0xC7: {                              /* mov r/m, imm32 */
        BYTE modrm = p[1];
        if ((modrm & 0xC0) == 0xC0) return 6; /* reg */
        if ((modrm & 0x07) == 0x05) return 10; /* [disp32] */
        if ((modrm & 0xC0) == 0x40) return 7;  /* [reg+disp8] */
        if ((modrm & 0xC0) == 0x80) return 10; /* [reg+disp32] */
        if ((modrm & 0x07) == 0x04) return 7;  /* SIB + disp8? */
        return 6;
    }
    case 0x8B: case 0x89: case 0x8A: case 0x88: /* mov reg<->r/m */
    case 0x3B: case 0x39: case 0x3A: case 0x38: /* cmp */
    case 0x83: case 0x81: case 0x03: case 0x01: /* add/cmp imm */
    case 0x85: case 0x84: case 0x87: case 0x86: /* test/xchg */
    case 0x33: case 0x31: case 0x2B: case 0x29: /* xor/sub */
    case 0xD1: case 0xD3: case 0xC1: case 0xFF: /* shift/inc */
    case 0x8D: case 0x69: case 0x6B:            /* lea/imul */
    case 0xF7: case 0xF6: {                     /* test/mul/not */
        BYTE modrm = p[1];
        if ((modrm & 0xC0) == 0xC0) return 2;
        if ((modrm & 0x07) == 0x05) return 6;
        if ((modrm & 0xC0) == 0x40) return 3;
        if ((modrm & 0xC0) == 0x80) return 6;
        return 2;
    }
    case 0xA1: case 0xA3: return 5;            /* mov eax,[moffs] */
    case 0x70: case 0x71: case 0x72: case 0x73: case 0x74:
    case 0x75: case 0x76: case 0x77: case 0x78: case 0x79:
    case 0x7A: case 0x7B: case 0x7C: case 0x7D: case 0x7E: case 0x7F:
        return 2;                               /* jcc rel8 */
    case 0xE1: case 0xE2: case 0xE3: return 2;  /* loop/jcxz */
    case 0x80: {                                /* cmp r/m, imm8 */
        BYTE modrm = p[1];
        if ((modrm & 0xC0) == 0xC0) return 3;
        if ((modrm & 0x07) == 0x05) return 7;
        if ((modrm & 0xC0) == 0x40) return 4;
        if ((modrm & 0xC0) == 0x80) return 7;
        return 3;
    }
    default:
        return 1;   /* 未知: 保守返回1(可能导致截断, 但函数头极少见未知指令) */
    }
}

/* 计算覆盖长度: 从 5 字节起, 延伸到完整指令边界 (避免截断) */
static int calc_hook_len(DWORD ea)
{
    int len = 0;
    while (len < 5) {
        int l = insn_len((BYTE *)(ea + len));
        if (l <= 0) l = 1;
        len += l;
    }
    return len;
}

/* 首指令安全性检查: 拒绝以跳转/返回开头(避免截断) */
static int first_insn_safe(DWORD ea)
{
    BYTE b0 = *(BYTE *)ea;
    if (b0 == 0xE9 || b0 == 0xE8 || b0 == 0xEB || b0 == 0xC3 || b0 == 0xC2)
        return 0;
    return 1;
}

/* 目标地址必须已映射且可写(防止版本不符/非游戏进程时崩溃) */
static int target_writable(DWORD ea)
{
    MEMORY_BASIC_INFORMATION mbi;
    DWORD prot;
    if (VirtualQuery((LPCVOID)ea, &mbi, sizeof(mbi)) == 0)
        return 0;
    if (mbi.State != MEM_COMMIT)
        return 0;
    prot = mbi.Protect;
    if (prot & (PAGE_NOACCESS | PAGE_EXECUTE_READ | PAGE_EXECUTE | PAGE_READONLY))
        return 0;   /* 无写权限 */
    return 1;
}

int hooks_install(const DWORD *targets, int count)
{
    int i, ok = 0;
    int base = g_hookCount;   /* 追加式: 可多次调用 */

    if (base + count > VIB_MAX_HOOKS) count = VIB_MAX_HOOKS - base;
    if (count <= 0) return 0;

    for (i = 0; i < count; i++) {
        HookCtx *h = &g_hooks[base + i];
        DWORD stubAddr = (DWORD)_vib_stubs[base + i];
        DWORD target = targets[i];
        int len;

        if (!target || !target_writable(target) || !first_insn_safe(target)) {
            h->installed = 0;
            continue;
        }
        len = calc_hook_len(target);
        if (len < 5 || len > 16) { h->installed = 0; continue; }
        h->target = target;
        h->stub = (void *)stubAddr;
        h->trampVar = g_trampVars[base + i];
        h->hookLen = len;
        memcpy(h->original, (void *)target, len);
        h->patchRel = stubAddr - target - 5;
        h->installed = 1;
        ok++;
    }

    /* 分配 trampoline 页(仅一次): 每 hook 32 字节 = 原指令(<=16) + E9 回跳 */
    if (ok > 0) {
        if (!g_trampPage) {
            g_trampPage = (BYTE *)VirtualAlloc(NULL, VIB_MAX_HOOKS * 32,
                                               MEM_COMMIT | MEM_RESERVE,
                                               PAGE_EXECUTE_READWRITE);
            if (!g_trampPage) {
                g_hookCount = base;
                return 0;
            }
        }
        for (i = 0; i < count; i++) {
            HookCtx *h = &g_hooks[base + i];
            if (!h->installed) continue;
            BYTE *t = g_trampPage + (base + i) * 32;
            int len = h->hookLen;
            memcpy(t, h->original, len);
            t[len] = 0xE9;
            *(DWORD *)(t + len + 1) = h->target + len - (DWORD)(t + len + 5);
            *h->trampVar = t;
        }
        /* 写 patch: E9 rel32 + 多余字节填 NOP */
        for (i = 0; i < count; i++) {
            HookCtx *h = &g_hooks[base + i];
            BYTE *p;
            int j;
            if (!h->installed) continue;
            p = (BYTE *)h->target;
            p[0] = 0xE9;
            *(DWORD *)(p + 1) = h->patchRel;
            for (j = 5; j < h->hookLen; j++)
                p[j] = 0x90;   /* NOP 填充 */
        }
        g_hookCount = base + count;
        g_installed = 1;
    }
    return ok;
}

void hooks_uninstall(void)
{
    int i;
    for (i = 0; i < g_hookCount; i++) {
        HookCtx *h = &g_hooks[i];
        if (h->installed && h->target) {
            memcpy((void *)h->target, h->original, h->hookLen);
            h->installed = 0;
        }
    }
    g_installed = 0;
}

int hooks_active(void)
{
    return g_installed;
}

/* 供 EXE 校验: 返回某 hook 的原始目标地址 */
DWORD hooks_target(int index)
{
    if (index < 0 || index >= g_hookCount) return 0;
    return g_hooks[index].target;
}
/* ---------------- 评分等级采集 (内存轮询版) ----------------
 * 不 hook 任何代码! 轮询读取评分等级:
 *   评分对象 = dword_4187B54 (全局指针)
 *   等级 = sub_1DD2100(评分对象, sub_1DD4D60(评分对象))
 * 直接调用游戏函数(游戏进程内), 零 UI 影响
 * 等级变化才发 (结算动画逐级变化) */
#define RANK_OBJ_PTR  0x04187B54
#define FN_1DD4D60    0x1DD4D60   /* 取伤害: thiscall(评分对象) -> 伤害值 */
#define FN_1DD2100    0x1DD2100   /* 等级判定: thiscall(评分对象, 伤害) -> 2~8 */

/* ---------------- 评分细分事件地址 (见 docs/评分细分事件采集地址总表_v5.md) ---------------- */
#define N2500_PTR       0x04294CE8   /* n2500 玩家对象全局指针 */
#define UI_MGR_PTR      0x04189AC0   /* ui_manager_frame_owner_global UI管理器指针 */
#define FN_GET_EXTPOOL  0x2F97E50   /* GetExtensionPool: thiscall(ui_mgr) -> UI对象 */
#define VT_DODGE_OFF    0x358        /* n2500 vtable+856: 极限闪避 thiscall(n2500)->bool */
#define SCORE_FINAL_KILL 3084        /* 评分对象偏移: 评分点累加器(明文, 每次命中累加) */
#define VT_COMBO_OFF    0x62C        /* n2500 vtable+1580: 连击容器 thiscall(n2500)->ptr */
#define ENC_TABLE_BASE  0x42D43E0    /* 加密表基址 (XOR 解密, sub_2C66980 用) */
#define VT_SLOT_OFF     0xBD4        /* n2500 vtable+3028: 场景槽号 thiscall(n2500)->int */
#define FN_1DD3900      0x1DD3900    /* 槽评分: thiscall(评分对象, float slot, uint idx) -> double */

typedef int (__attribute__((thiscall)) *FnGetDmg)(int thisptr);
typedef int (__attribute__((thiscall)) *FnGetRank)(int thisptr, int dmg);
typedef unsigned char (__attribute__((thiscall)) *FnVtDodge)(int thisptr);
typedef void* (__attribute__((thiscall)) *FnGetExtPool)(int thisptr);
typedef int (__attribute__((thiscall)) *FnVtCombo)(int thisptr);
typedef int (__attribute__((thiscall)) *FnVtSlot)(int thisptr);
typedef double (__attribute__((thiscall)) *FnGetSlot)(int thisptr, float slot, unsigned int idx);

static volatile LONG g_rankPollLast = 0;   /* 上次等级 */
static volatile LONG g_rankPollCnt = 0;    /* 变化次数 */

static void poll_kill(DWORD now);
static void poll_move(DWORD now);

static DWORD try_read_dword(DWORD addr)
{
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery((LPCVOID)addr, &mbi, sizeof(mbi)) == 0)
        return 0;
    if (mbi.State != MEM_COMMIT)
        return 0;
    if (mbi.Protect & (PAGE_NOACCESS | PAGE_GUARD))
        return 0;
    return *(volatile DWORD *)addr;
}

/* 聚合线程调用: 轮询评分等级, 变化时发送 */
void rank_poll(void)
{
    static DWORD s_lastDbg = 0;
    DWORD now = GetTickCount();
    DWORD obj = try_read_dword(RANK_OBJ_PTR);
    if (!obj) {
#ifdef VIB_DEBUG_TXT
        if (now - s_lastDbg >= 500) {
            s_lastDbg = now;
            dll_log2("RANK(poll) obj=NULL (评分对象未创建)");
        }
#endif
        return;
    }
    /* 直接调用游戏函数取评分等级 (游戏进程内) */
    DWORD lvl = 0;
    if (obj) {
        FnGetDmg fnDmg = (FnGetDmg)FN_1DD4D60;
        FnGetRank fnRank = (FnGetRank)FN_1DD2100;
        int dmg = fnDmg((int)obj);
        int r = fnRank((int)obj, dmg);
        lvl = (DWORD)r;
    }
#ifdef VIB_DEBUG_TXT
    if (now - s_lastDbg >= 500) {
        s_lastDbg = now;
        dll_log2("RANK(poll) obj=0x%08X lvl=%lu", obj, lvl);
    }
#endif
    if (lvl > 8) return;
    LONG prev = g_rankPollLast;
    if (prev == (LONG)lvl) return;
    if (InterlockedCompareExchange(&g_rankPollLast, (LONG)lvl, prev) != prev)
        return;
    InterlockedIncrement(&g_rankPollCnt);
    vib_collect(VEV_RANKING, lvl, GetTickCount(), 1);
#ifdef VIB_DEBUG_TXT
    dll_log2("RANK(poll) level=%lu obj=0x%08X (SENT)", lvl, obj);
#endif
}

/* 兼容旧接口名: 统一走轮询 */
void vib_flush_rank(void)
{
    rank_poll();
}

/* ---------------- 评分细分事件轮询 (零 hook, 聚合线程, 只读 getter) ----------------
 * 原则: 与 rank_poll 同构。hook 会破坏 UI (已验证), 故全部走:
 *   读只读字段 (try_read_dword) + 直接调用只读 vtable getter
 * 地址均经 IDA 反汇编确认调用约定:
 *   闪避 = (*n2500+856)(n2500)   (sub_1DD4D60 同款 vtable 调用范式)
 *   最终击杀 = [[0x4187B54]+3084] (简单 dword)
 *   暴击/破招/背击 = GetExtensionPool([0x4189AC0]) -> UI对象 -> +3828/3829/3830/3831 */

static volatile LONG g_dodgeLast = 0;
static volatile DWORD g_uiObj = 0;
static volatile DWORD g_scoreLast = 0;
static volatile LONG g_armorLast = 0;
static volatile LONG g_aerialLast = 0;

/* 连击数: 不从内存解密 (vtable+1580 容器是累计命中计数, 非实时连击)。
 * 连击数由命中事件累加 (引擎 combo_hits 已有, 1500ms 无命中清零)。 */

/* 破甲(槽4/5) + 凌空追击(槽10/14) 槽评分增量采集。
 * 调用 sub_1DD3900 (游戏函数, 内部解密) 读槽评分值, 变化即触发。
 * 注意: slot 参数是整数位模式 (COERCE_FLOAT), 传真实 float 会触发断言崩溃 */
static float slot_bits(int n)
{
    union { int i; float f; } u;
    u.i = n;
    return u.f;
}

static void poll_armor_aerial(DWORD score, DWORD player, DWORD now)
{
    DWORD vt = try_read_dword(player);
    if (!vt) return;
    DWORD fnaddr = try_read_dword(vt + VT_SLOT_OFF);
    if (!fnaddr) return;
    int slot = ((FnVtSlot)fnaddr)((int)player);
    if (slot < 0 || slot > 7) return;
    FnGetSlot fnSlot = (FnGetSlot)FN_1DD3900;
    if (!fnSlot) return;

    LONG armor = (LONG)(fnSlot((int)score, slot_bits(4), (unsigned)slot)
              + fnSlot((int)score, slot_bits(5), (unsigned)slot));
    LONG aerial = (LONG)(fnSlot((int)score, slot_bits(10), (unsigned)slot)
               + fnSlot((int)score, slot_bits(14), (unsigned)slot));

    if (armor != g_armorLast) {
        LONG prev = g_armorLast;
        g_armorLast = armor;
        if (armor > prev && armor > 0) {
            vib_collect(VEV_ARMOR_BREAK, (DWORD)(armor - prev), now, 1);
#ifdef VIB_DEBUG_TXT
            dll_log2("ARMOR(poll) +%ld (total=%ld)", armor - prev, armor);
#endif
        }
    }
    if (aerial != g_aerialLast) {
        LONG prev = g_aerialLast;
        g_aerialLast = aerial;
        if (aerial > prev && aerial > 0) {
            vib_collect(VEV_AERIAL, (DWORD)(aerial - prev), now, 1);
#ifdef VIB_DEBUG_TXT
            dll_log2("AERIAL(poll) +%ld (total=%ld)", aerial - prev, aerial);
#endif
        }
    }
}

void vib_flush_rank_extra(void)
{
    DWORD now = GetTickCount();
    DWORD score = try_read_dword(RANK_OBJ_PTR);
    DWORD player = try_read_dword(N2500_PTR);

    /* 0. 怪物死亡(击杀) + 移动 采集 (任务②④) */
    poll_kill(now);
    poll_move(now);

    /* 1. 极限闪避: (*n2500+856)(n2500) == 1 上升沿
     * 开局延迟 3s: vtable+856 开局会短暂返回 1 (加载/无敌帧), 排除假阳性 */
    {
        static DWORD s_startTick = 0;
        if (!s_startTick) s_startTick = now;
        if (now - s_startTick > 3000 && player) {
            DWORD vt = try_read_dword(player);
            if (vt) {
                DWORD fnaddr = try_read_dword(vt + VT_DODGE_OFF);
                if (fnaddr) {
                    unsigned char dodge = ((FnVtDodge)fnaddr)((int)player);
                    LONG prev = g_dodgeLast;
                    if (dodge == 1 && prev == 0) {
                        vib_collect(VEV_DODGE, 1, now, 1);
#ifdef VIB_DEBUG_TXT
                        dll_log2("DODGE(poll) trigger");
#endif
                    }
                    g_dodgeLast = dodge;
                }
            }
        }
    }

    /* 2. 评分点: score+3084 = 评分点累加器 (明文, 每次命中累加)
     * 只发增量(正), 避免刷屏 */
    if (score) {
        DWORD fk = try_read_dword(score + SCORE_FINAL_KILL);
        if (fk < 100000000u) {
            DWORD last = g_scoreLast;
            if (fk != last) {
                g_scoreLast = fk;
                if (fk > last && fk > 0) {
                    vib_collect(VEV_KILLPOINT, fk - last, now, 1);
#ifdef VIB_DEBUG_TXT
                    dll_log2("SCOREPOINT(poll) +%lu (total=%lu)", fk - last, fk);
#endif
                }
            }
        }
    }

    /* 3. 破甲(槽4/5) + 凌空追击(槽10/14) 槽评分增量 */
    if (score && player) {
        poll_armor_aerial(score, player, now);
    }

    /* 4. 暴击/破招/背击: UI 对象类型字段 (暂缓, 字段战斗中恒 0) */
    {
        DWORD ui_mgr = try_read_dword(UI_MGR_PTR);
        if (ui_mgr) {
            if (!g_uiObj || !ptr_valid((DWORD)g_uiObj + 3831)) {
                g_uiObj = (DWORD)((FnGetExtPool)FN_GET_EXTPOOL)((int)ui_mgr);
            }
            if (g_uiObj && ptr_valid(g_uiObj + 3831)) {
                DWORD t3828 = *(volatile BYTE *)(g_uiObj + 3828);
                DWORD t3829 = *(volatile BYTE *)(g_uiObj + 3829);
                DWORD t3830 = *(volatile BYTE *)(g_uiObj + 3830);
                DWORD t3831 = *(volatile BYTE *)(g_uiObj + 3831);
                /* 暂缓: 字段战斗中恒 0, 无法区分暴击/破招/背击 */
                (void)t3828; (void)t3829; (void)t3830; (void)t3831;
            }
        }
    }
}

/* ---------------- 任务②: 怪物死亡(击杀)采集 ----------------
 * n2500[5747] (玩家对象偏移 0x59CC) = 击杀计数
 * sub_1D1E9F0 (玩家 vtable 槽40) 每次击杀 +1, sub_1DD9960 每秒冲刷清零
 * 轮询增量, 零 hook */
#define PLAYER_OBJ_PTR   0x04294CE8   /* n2500 */
#define KILL_COUNT_OFF   0x59CC       /* n2500[5747] */

static volatile DWORD g_killLast = 0;

static void poll_kill(DWORD now)
{
    DWORD player = try_read_dword(PLAYER_OBJ_PTR);
    if (!player) { g_killLast = 0; return; }
    DWORD cnt = try_read_dword(player + KILL_COUNT_OFF);
    if (cnt >= 0x7FFFFFFFu) return;
    DWORD last = g_killLast;
    g_killLast = cnt;
    if (cnt > last && cnt > 0) {
        vib_collect(VEV_KILL, cnt - last, now, 1);
#ifdef VIB_DEBUG_TXT
        dll_log2("KILL(poll) +%lu (total=%lu)", cnt - last, cnt);
#endif
    }
}

/* ---------------- 任务④: 移动采集 ----------------
 * n964 (0x418A458) = 玩家位置: +4=X(float) +8=Y(float)
 * sub_1E3B7D0 每帧更新 (明文像素坐标), 纯读全局, 零风险 */
#define N964_POS        0x0418A458   /* 玩家位置对象 */
#define MOVE_SPEED_OFF   0x9A8        /* 移动速度(加密, offset 4) */
#define DECODE_TBL_BASE  0x042D43E0   /* DecodeAdr */
#define MOVE_STOP_MS     300          /* 停止判定超时 */

static volatile float g_moveX = 0.0f, g_moveY = 0.0f;
static volatile DWORD g_lastMoveTick = 0;
static volatile int  g_moving = 0;
static volatile int  g_moveInit = 0;

/* 用户提供 decode 算法: 加密字段 -> 明文 */
static int vib_decode(DWORD adr, unsigned int offset)
{
    DWORD enc = try_read_dword(adr);
    DWORD hi = enc >> 16;
    DWORD lo = enc & 0xFFFF;
    DWORD base1 = try_read_dword(DECODE_TBL_BASE);
    if (!base1) return 0;
    DWORD tbl2 = try_read_dword(base1 + hi * 4 + 0x24);
    if (!tbl2) return 0;
    DWORD t = try_read_dword(tbl2 + lo * 4 + 0x2114);
    DWORD ax = t & 0xFFFF;
    DWORD v = (ax << 16) | ax;
    DWORD enc2 = try_read_dword(adr + offset);
    return (int)(v ^ enc2);
}

static void poll_move(DWORD now)
{
    DWORD px = try_read_dword(N964_POS + 4);
    DWORD py = try_read_dword(N964_POS + 8);
    float x = *(float *)&px;
    float y = *(float *)&py;
    if (!g_moveInit) {                    /* 首帧: 只记录基准, 不发 */
        g_moveX = x;
        g_moveY = y;
        g_moveInit = 1;
        return;
    }
    float dx = x - g_moveX;
    float dy = y - g_moveY;
    g_moveX = x;
    g_moveY = y;

    float dist = (dx < 0 ? -dx : dx) + (dy < 0 ? -dy : dy);
    if (dist >= 1.0f) {                   /* 像素级位移, >=1 即移动 */
        DWORD str = 40;                   /* 默认强度(无法解码时) */
        DWORD player = try_read_dword(PLAYER_OBJ_PTR);
        if (player) {
            int s = vib_decode(player + MOVE_SPEED_OFF, 4);
            if (s > 0) {
                if (s > 2000) s = 2000;
                str = (DWORD)((unsigned)s * 100u / 2000u);
                if (str < 5) str = 5;
                if (str > 100) str = 100;
            }
        }
        vib_collect(VEV_MOVE, str, now, 0);
        g_moving = 1;
        g_lastMoveTick = now;
    } else if (g_moving && now - g_lastMoveTick > MOVE_STOP_MS) {
        vib_collect(VEV_MOVE, 0, now, 0);
        g_moving = 0;
    }
}

/* ---------------- 震屏采集 (hook sub_1E25540 / sub_1E268F0) ----------------
 * sub_1E25540 = 相机震动汇聚点 (所有震屏: 技能命中/释放/死亡等)
 * sub_1E268F0 = [shake screen] 词条唯一转发入口 (纯镜头震动)
 * 两者分别发 VEV_SKILL_HIT / VEV_SHAKE_SCREEN
 * 强度平方根放大(感知均匀) + 100ms 去重 */
static volatile DWORD g_shakeRecent = 0;

static DWORD shake_amplify(float f)
{
    DWORD s;
    if (f <= 0.0f) f = 0.0f;
    if (f > 1.0f) f = 1.0f;
    s = (DWORD)(sqrtf(f) * 100.0f);   /* 平方根放大: 小强度充分展开 */
    if (s < 25) s = 25;               /* 保底 */
    if (s > 100) s = 100;
    return s;
}

/* [shake screen] 纯镜头震动: hook sub_1E268F0 */
void __cdecl vib_dispatch_shake(void *obj, DWORD strength_bits, DWORD time_ms, int type)
{
    DWORD now;
    (void)obj; (void)type;
    now = GetTickCount();
    g_shakeRecent = now;
    /* 【镜头震动】已搁置 (2026-08-15): 用户实测 sub_1E268F0 条件苛刻难触发, 暂不发送
     * 保留 hook 采集点: sub_1E268F0 = 真 [shake screen] 词条转发唯一入口
     * 将来恢复: 放开下方 vib_collect 即可 (VEV_SHAKE_SCREEN -> idx 10) */
    // vib_collect(VEV_SHAKE_SCREEN, shake_amplify(*(float *)&strength_bits), now, time_ms);
#ifdef VIB_DEBUG_TXT
    dll_log2("SHAKE(hook) raw=%.3f str=%lu time=%lu", *(float *)&strength_bits, shake_amplify(*(float *)&strength_bits), time_ms);
#endif
}

/* 技能震动 (广义相机震动): hook sub_1E25540, 按返回地址分类
 * - 击杀/死亡特写 (sub_1CB80A0 击杀 / sub_1D30270 case15) -> VEV_CRIT_SHAKE
 * - 相机帧更新重放 (sub_1E26800) / [shake screen] 转发 (sub_1E268F0) -> 忽略(去重)
 * - 其余 (技能函数硬编码) -> VEV_SKILL_HIT */
void __cdecl vib_dispatch_skillhit(void *obj, DWORD strength_bits, DWORD time_ms, DWORD retaddr, int type)
{
    DWORD now;
    (void)obj; (void)type;
    now = GetTickCount();

    /* 相机系统内部调用 -> 忽略(去重):
     *   sub_1E26800(帧重放) + sub_1E268F0(stub9 转发)
     *   sub_1E24DE0(震屏结束收尾)
     *   sub_2C56xxx(震屏状态机 开始/持续/结束) */
    if (retaddr >= 0x01E26800 && retaddr < 0x01E26900) return;
    if (retaddr >= 0x01E24E00 && retaddr < 0x01E24F00) return;
    if (retaddr >= 0x02C56C00 && retaddr < 0x02C57000) return;

    if ((retaddr >= 0x01CB8000 && retaddr < 0x01CB9000) ||   /* sub_1CB80A0 击杀特写 */
        (retaddr >= 0x01D30200 && retaddr < 0x01D31000)) {   /* sub_1D30270 死亡case15 */
        vib_collect(VEV_CRIT_SHAKE, shake_amplify(*(float *)&strength_bits), now, time_ms);
#ifdef VIB_DEBUG_TXT
        dll_log2("CRIT_SHAKE(hook) raw=%.3f time=%lu ret=0x%08X", *(float *)&strength_bits, time_ms, retaddr);
#endif
        return;
    }
    /* 【技能震动】已搁置 (2026-08-15): 普通技能震屏路径未定位 (Omnislay 不走相机字段), 暂不发送
     * 保留 hook 采集点: sub_1E25540 = 相机震动汇聚点 (46 调用者)
     * 将来恢复: 放开下方 vib_collect 即可 (VEV_SKILL_HIT -> idx 12)
     * 参考 docs/镜头震动定位_v15.md */
    // vib_collect(VEV_SKILL_HIT, shake_amplify(*(float *)&strength_bits), now, time_ms);
#ifdef VIB_DEBUG_TXT
    dll_log2("SKILLHIT(hook) raw=%.3f time=%lu ret=0x%08X", *(float *)&strength_bits, time_ms, retaddr);
#endif
}

/* 读条震屏 (暴走等 buff 读条结束): hook sub_1E25760
 * 标志 a3 低字节 == 1 (开始震屏) 时采集, 时间硬编码 200ms */
void __cdecl vib_dispatch_readshake(void *obj, DWORD flag_bits, DWORD strength_bits, int type)
{
    DWORD now;
    (void)obj; (void)type;
    if ((BYTE)flag_bits != 1) return;   /* 只采"开始震屏"(0=停止) */
    now = GetTickCount();
    g_shakeRecent = now;
    /* 【镜头震动】已搁置 (2026-08-15): 读条震屏归入镜头震动一并搁置, 暂不发送
     * 保留 hook 采集点: sub_1E25760 = buff 读条结束震屏 setter
     * 将来恢复: 放开下方 vib_collect 即可 (VEV_SHAKE_SCREEN -> idx 10)
     * 参考 docs/镜头震动真相_v13.md */
    // vib_collect(VEV_SHAKE_SCREEN, shake_amplify(*(float *)&strength_bits), now, 200);
#ifdef VIB_DEBUG_TXT
    dll_log2("READSHAKE(hook) str=%.3f time=200", *(float *)&strength_bits);
#endif
}

/* 怪物死亡采集: hook sub_F20BF0 (仅在被击者 HP<=0 确认死亡后由 sub_1C58AC0 调用)
 * 之前 hook sub_1C58AC0 入口误报 (受击高频 + 参数语义不一), 改 hook 死亡后处理
 * 玩家对象地址缓存(游戏进程内固定), 避免每次调用做 VirtualQuery */
static volatile DWORD g_n2500Cache = 0;

void __cdecl vib_dispatch_targetdie(void *obj, int type)
{
    DWORD now;
    (void)type;
    if ((DWORD)obj == 0)
        return;                          /* 无效对象: 忽略 */
    if (!g_n2500Cache)
        g_n2500Cache = try_read_dword(PLAYER_OBJ_PTR);
    if (g_n2500Cache && (DWORD)obj == g_n2500Cache)
        return;                          /* 玩家自身 HP 归零: 忽略 */
    now = GetTickCount();
    vib_collect(VEV_TARGET_DIE, 100, now, 1);
#ifdef VIB_DEBUG_TXT
    dll_log2("TARGET_DIE(hook) victim=0x%08X", (DWORD)obj);
#endif
}

/* 直接震屏采集: hook sub_1E26F00 (直接写相机震屏字段)
 * 技能/场景"目标容器为空"时调用 (普通技能不命中也震), 时间固定 200ms
 * 100ms 去重 (与 g_shakeRecent 同源, 防连发刷屏) */
void __cdecl vib_dispatch_directshake(void *obj, DWORD strength_bits, int type)
{
    DWORD now;
    (void)obj; (void)type;
    now = GetTickCount();
    if (now - g_shakeRecent < 100) return;
    g_shakeRecent = now;
    /* 【镜头震动】已搁置 (2026-08-15): 直接震屏归入镜头震动一并搁置, 暂不发送
     * 保留 hook 采集点: sub_1E26F00 = 直接写相机震屏字段 (目标容器空时, 不命中也震)
     * 将来恢复: 放开下方 vib_collect 即可 (VEV_SHAKE_SCREEN -> idx 10) */
    // vib_collect(VEV_SHAKE_SCREEN, shake_amplify(*(float *)&strength_bits), now, 200);
#ifdef VIB_DEBUG_TXT
    dll_log2("DIRECTSHAKE(hook) raw=%.3f", *(float *)&strength_bits);
#endif
}

/* ---------------- 相机震屏轮询 (最终效果监控, 覆盖所有震屏路径) ----------------
 * 相机对象 = dword_4158A38, +1952 = 当前震屏强度(float)
 * 任何震屏 (技能/词条/读条/暴击) 最终都写这里 → 上升沿(0→非0) 即震屏开始
 * 发 VEV_SKILL_HIT (技能震动), 强度=当前值 */
#define CAM_OBJ_PTR     0x04158A38   /* 相机对象全局指针 */
#define CAM_SHAKE_STR   1952         /* 当前震屏强度 float */

static volatile float g_camShakeLast = 0.0f;

void poll_cam_shake(void)
{
    DWORD now, cam;
    float s;
    cam = try_read_dword(CAM_OBJ_PTR);
    if (!cam) { g_camShakeLast = 0.0f; return; }
    s = *(volatile float *)(cam + CAM_SHAKE_STR);
    /* 上升沿: 0 → 非0 (震屏开始); 及强度显著变化 (连续震屏) */
    if (s > 0.01f) {
        if (g_camShakeLast <= 0.01f || s != g_camShakeLast) {
            now = GetTickCount();
            /* 【技能震动】已搁置 (2026-08-15): 相机震屏轮询 (最终效果监控) 一并搁置
             * 覆盖所有震屏路径 (技能/词条/读条/暴击都写 +1952), 但 Omnislay 实测不写相机字段
             * 将来恢复: 放开下方 vib_collect 即可 (VEV_SKILL_HIT -> idx 12)
             * 注意: 与 stub 8 的 CRIT_SHAKE 可能重复 (暴击时两者都检测到) */
            // vib_collect(VEV_SKILL_HIT, shake_amplify(s), now, 200);
#ifdef VIB_DEBUG_TXT
            dll_log2("CAM_SHAKE(poll) s=%.3f", s);
#endif
        }
    }
    g_camShakeLast = s;
}

/* ---------------- 背击/破招定位 (诊断, 2026-08-15) ----------------
 * 用户提供触发条件:
 *   背击 = 玩家朝向与怪物朝向相同 (back attack)
 *   破招 = 受击对象处于攻击/被控制状态时被攻击 (counter)
 * 定位: 直接调用 nkpi_key_unwrap 曾导致崩溃, 改为 DLL 内模拟解密 (v7 文档算法)
 * 参考: docs/暴击破招背击判定地址.md / docs/背击破招定位_v18.md */

/* 模拟 nkpi_key_unwrap (v7 文档 sub_2C64C90 算法), 纯计算不调用游戏函数 */
static void sim_unwrap(DWORD kp, wchar_t *out, int outsz)
{
    int o = 0;
    if (!kp || !out || outsz <= 0) return;
    out[0] = 0;
    if (*(volatile WORD *)kp == 11425) {
        /* 明文内联在 kp+2 */
        const unsigned __int16 *p = (const unsigned __int16 *)(kp + 2);
        while (o < outsz - 1 && p[o]) { out[o] = p[o]; o++; }
        out[o] = 0;
        return;
    }
    /* 加密: sub_2C64C90 算法 */
    {
        BYTE v7 = *(volatile BYTE *)(kp + 1) & 0xFE;
        DWORD dlen = 2 * (*(volatile WORD *)(kp + 1) ^ (v7 | (v7 << 7)));
        DWORD key = v7 | 0x9A714CA0;
        DWORD pos = 0;
        BYTE buf[256];
        if (dlen > sizeof(buf)) dlen = sizeof(buf);
        while (pos + 4 <= dlen) {
            DWORD d = *(volatile DWORD *)(kp + pos);
            DWORD outv = key ^ d;
            buf[pos] = (BYTE)(outv & 0xFF);
            buf[pos + 1] = (BYTE)((outv >> 8) & 0xFF);
            buf[pos + 2] = (BYTE)((outv >> 16) & 0xFF);
            buf[pos + 3] = (BYTE)((outv >> 24) & 0xFF);
            key = (outv + 65599 * key) & 0xFFFFFFFF;
            pos += 4;
        }
        while (pos < dlen) {
            BYTE b = *(volatile BYTE *)(kp + pos);
            DWORD outv = key ^ b;
            buf[pos] = (BYTE)(outv & 0xFF);
            key = (outv + 263 * key) & 0xFFFFFFFF;
            pos++;
        }
        /* UTF-16LE 转 wchar */
        o = 0;
        while (o + 1 < dlen && o < outsz - 1) {
            out[o] = (wchar_t)(buf[o] | (buf[o + 1] << 8));
            o++;
        }
        out[o] = 0;
    }
}

/* 解密 11 个评分文字 key 输出明文 (进图后, 标定背击/破招) */
void diag_rank_texts(void)
{
    static const DWORD keys[11] = {
        0x39A72AC, 0x39A7290, 0x39A72C8, 0x39A732C, 0x39A7388,
        0x39A73BC, 0x39A73E0, 0x39A7418, 0x39A7434, 0x39A7454, 0x39A7488
    };
    static int s_done = 0;
    int i;
    if (s_done) return;
    s_done = 1;
    for (i = 0; i < 11; i++) {
        DWORD kp = try_read_dword(keys[i]);
        if (!kp) continue;
        {
            wchar_t txt[64];
            int j;
            BYTE hex[32];
            for (j = 0; j < 32; j++) hex[j] = *(volatile BYTE *)(kp + j);
            sim_unwrap(kp, txt, 64);
            dll_log2("RANKTXT key[%d] store=0x%08X ptr=0x%08X hex=%02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X %02X w0=0x%04X = %ls",
                     i, keys[i], kp,
                     hex[0], hex[1], hex[2], hex[3], hex[4], hex[5], hex[6], hex[7],
                     hex[8], hex[9], hex[10], hex[11], hex[12], hex[13], hex[14], hex[15],
                     *(volatile WORD *)kp, txt);
        }
    }
}

/* ---------------- 评分事件计数器轮询 (背击/破招定位 v18) ----------------
 * 评分对象 = dword_4187B54, 事件计数器 = score + 292*槽 + 136 + 8*etype (加密)
 * sub_2C66980 (加密读) 算法与 vib_decode 一致 (表 0x42D43E0 + 0x24 + 0x2114)
 * 用户打出背击/破招时, 观察哪个 etype 增量 → 确定事件类型 (静态分析已穷尽) */
#define SCORE_PTR 0x04187B54

static volatile int g_evLast[17];

void poll_rank_events(void)
{
    DWORD score, player;
    int slot, e;
    score = try_read_dword(SCORE_PTR);
    player = try_read_dword(PLAYER_OBJ_PTR);
    if (!score || !player) return;
    {
        DWORD vt = try_read_dword(player);
        if (!vt) return;
        DWORD fnaddr = try_read_dword(vt + VT_SLOT_OFF);
        if (!fnaddr) return;
        slot = ((FnVtSlot)fnaddr)((int)player);
    }
    if (slot < 0 || slot > 7) return;
    for (e = 0; e < 17; e++) {
        DWORD adr = score + 292 * slot + 136 + 8 * e;
        int v = vib_decode(adr, 4);
        if (v != g_evLast[e]) {
            if (v > g_evLast[e] && v > 0)
                dll_log2("RANKEVT etype=%d +%d (total=%d)", e, v - g_evLast[e], v);
            g_evLast[e] = v;
        }
    }
}

/* ---------------- 破招/背击文字扫描 (v18b, 只读内存, 零风险) ----------------
 * 评分 UI 显示"背击(back attack)/破招(counter)"文字, 来自 dstr 明文区 (0x2AXX5400)
 * 代码不直接引用明文地址, 而是 nkpi_key_unwrap(dword_39A7XXX) (key 指针存储处)
 * 步骤: 扫描明文找字符串 → 读 [addr-2] 首 word (11425=明文) 得 key 指针
 *       → 匹配 11 个 key 存储处 (dword_39A72AC 等) → IDA 搜引用该 dword 的代码 */
static void scan_match_key(DWORD str_addr, const char *tag)
{
    static const DWORD keyStores[11] = {
        0x39A72AC, 0x39A7290, 0x39A72C8, 0x39A732C, 0x39A7388,
        0x39A73BC, 0x39A73E0, 0x39A7418, 0x39A7434, 0x39A7454, 0x39A7488
    };
    int i;
    DWORD key = 0;
    if (str_addr >= 2) {
        WORD w0 = *(volatile WORD *)(str_addr - 2);
        if (w0 == 11425)      /* 明文标志: key 指针 = addr-2 */
            key = str_addr - 2;
    }
    for (i = 0; i < 11; i++) {
        DWORD v = try_read_dword(keyStores[i]);
        if (v && (v == key || (key == 0 && v < 0x2B000000 && v >= 0x2A000000))) {
            /* 匹配或同区候选 */
            dll_log2("DSTR %s @ 0x%08X key=0x%08X store=dword_%08X", tag, str_addr, key, keyStores[i]);
        }
    }
}

void scan_dstr_text(void)
{
    MEMORY_BASIC_INFORMATION mbi;
    BYTE *addr = (BYTE *)0x2A000000;
    BYTE *end = (BYTE *)0x2B000000;
    static const struct { const char *ascii; const char *tag; } needles[] = {
        { "back", "BACK" }, { "counter", "COUNTER" }, { "break", "BREAK" },
        { "rear", "REAR" }, { "\xb4\xc6\xd5\xd0", "POZH" }, { "\xb1\xb3\xbb\xf7", "BEIJI" },
    };
    int k, i;
    while ((ULONG_PTR)addr < (ULONG_PTR)end) {
        if (VirtualQuery(addr, &mbi, sizeof(mbi)) == 0)
            break;
        if (mbi.State == MEM_COMMIT &&
            (mbi.Protect & (PAGE_READWRITE | PAGE_READONLY | PAGE_WRITECOPY))) {
            BYTE *base = (BYTE *)mbi.BaseAddress;
            SIZE_T sz = mbi.RegionSize;
            for (k = 0; k < (int)(sizeof(needles) / sizeof(needles[0])); k++) {
                size_t blen = strlen(needles[k].ascii);
                if (blen == 0) continue;
                BYTE pat[32];
                size_t p;
                for (p = 0; p < blen; p++) {
                    BYTE c = (BYTE)needles[k].ascii[p];
                    if (c >= 'A' && c <= 'Z') c += 32;   /* 统一小写 */
                    pat[p * 2] = c;
                    pat[p * 2 + 1] = 0;
                }
                for (i = 0; (SIZE_T)i + blen * 2 < sz; i++) {
                    size_t p2;
                    int match = 1;
                    for (p2 = 0; p2 < blen * 2; p2++) {
                        BYTE c = base[i + p2];
                        if (c >= 'A' && c <= 'Z') c += 32;   /* 大小写不敏感 */
                        if (c != pat[p2]) { match = 0; break; }
                    }
                    if (match) {
                        DWORD sa = (DWORD)(ULONG_PTR)(base + i);
                        dll_log2("DSTR %s @ 0x%08X", needles[k].tag, sa);
                        scan_match_key(sa, needles[k].tag);
                    }
                }
            }
        }
        addr = (BYTE *)mbi.BaseAddress + mbi.RegionSize;
        if ((ULONG_PTR)addr < 0x2A000000) addr = (BYTE *)0x2A000000;
    }
    dll_log2("DSTR scan done");
}

