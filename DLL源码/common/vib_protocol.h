#ifndef VIB_PROTOCOL_H
#define VIB_PROTOCOL_H
/* ============================================================
 * DFO 手柄震动插件 - 共享协议定义 v2
 * ============================================================ */
#include <windows.h>

#pragma pack(push, 1)

#define VIB_SHM_NAME       "Local\\DfoVibrationShm"
#define VIB_SHM_MAGIC      0x564F4656
#define VIB_SHM_VERSION    2
#define VIB_RING_SIZE      (256 * 1024)

/* ---- 事件类型(共享内存) ---- */
enum VibEventType {
    VEV_ATTACK      = 0,
    VEV_DAMAGE      = 1,
    VEV_TARGET_DIE  = 2,
    VEV_RESET_COMBO = 3,
    VEV_START_BATTLE= 4,
    VEV_ACTION_END  = 5,
    VEV_SHAKE_INPUT = 6,
    VEV_HP_LOST     = 7,
    VEV_RATING      = 8,   /* 评分特效: strength=幅度0-100(暴击/破招/背击/评级等) */
    VEV_COMBO       = 9,   /* 连击计数: 引擎叠加 10->60 */
    VEV_FONT        = 10,  /* 飘字事件: strength = a6 类型标志位(见下) */
    VEV_RANKING     = 11,  /* 评分等级写入: strength=评分等级(2~8, 8=特殊动作) */
    VEV_KILLPOINT   = 12,  /* 击杀点/评分点: strength=点数 */
    VEV_DODGE       = 13,  /* 极限闪避: strength=1 */
    VEV_CRIT        = 14,  /* 暴击: strength=1 */
    VEV_BREAK       = 15,  /* 破招: strength=1 */
    VEV_BACK        = 16,  /* 背击: strength=1 */
    VEV_FINAL_KILL  = 17,  /* 最终击杀: strength=击杀数 */
    VEV_AERIAL      = 18,  /* 凌空追击: strength=1 */
    VEV_ARMOR_BREAK = 19,  /* 破甲: strength=1 */
    VEV_BUFF_STACK  = 20,  /* 增益叠加: strength=层数 */
    VEV_KILL        = 21,  /* 释放技能: strength=次数 (n2500[5747]) */
    VEV_SHAKE_SCREEN= 22,  /* 真镜头震动 [shake screen]: strength=强度0-100, count=时间ms */
    VEV_MOVE        = 23,  /* 移动: strength=速度映射0-100(0=停止), 持续型 */
    VEV_SKILL_HIT   = 24,  /* 技能震动: strength=强度0-100, count=时间ms (技能释放/命中) */
    VEV_CRIT_SHAKE  = 25,  /* 暴击/击杀特写震屏: strength=强度0-100, count=时间ms */
};

/* 飘字 a6 类型标志位(源自 E03210 调用者) */
#define FONT_FLAG_PLAYER_ATTACK 0x01   /* 玩家攻击 */
#define FONT_FLAG_PLAYER_HIT    0x02   /* 玩家受击 */
#define FONT_FLAG_EFFECT        0x04   /* 特效飘字(随机偏移) */
#define FONT_FLAG_STATE         0x08   /* 状态类(无敌/霸体) */
#define FONT_FLAG_SPECIAL       0x10   /* 特殊(受击状态) */
#define FONT_FLAG_HP            0x20   /* HP数值飘字(双行) */
#define FONT_FLAG_OTHER         0x40   /* 其他/未知位 */

typedef struct {
    DWORD type;
    DWORD strength;
    DWORD tick;
    DWORD reserved;
} VibEvent;

typedef struct {
    volatile DWORD head;
    volatile DWORD tail;
    DWORD capacity;
    BYTE  data[VIB_RING_SIZE];
} VibRing;

typedef struct {
    DWORD magic;
    DWORD version;
    DWORD gamePid;
    DWORD flags;
    volatile DWORD seq;
    volatile DWORD lastTick;
    DWORD apiVersion[7];
    VibRing ring;
} VibShm;

#pragma pack(pop)

/* ---------------- 配置(TOML) ---------------- */
#define VIB_TOML_FILE      "DfoVibration.toml"
#define VIB_INI_FILE       "DfoVibration.ini"

#define MAP_MAX            64
#define MAP_TARGET_MAX     8

/* 触发源 */
enum {
    TRIG_XINPUT = 0,      /* XInput 标准键/摇杆方向, xid 有效 */
    TRIG_RAW    = 1,      /* RawInput 扩展键(背键/圆盘/滚轮), rawId 有效 */
};

/* 目标动作(每映射一个目标: 键盘组 或 鼠标动作) */
enum {
    TGT_KEYS   = 0,       /* targetKeys[0..n] 键盘 VK 序列 */
    TGT_MOUSEBTN = 1,     /* 鼠标按键: mouseId 1=左 2=右 3=中 4=X1 5=X2 */
    TGT_MOUSE_MOVE = 2,   /* 鼠标 8 向移动: moveDir 1-8, moveSpeed 像素/帧 */
    TGT_MOUSE_SCROLL = 3, /* 滚轮: scrollDir 1=上 2=下 */
};

typedef struct {
    DWORD src;              /* TRIG_XINPUT / TRIG_RAW */
    DWORD xid;              /* XInput 按钮 id (0x01-0x16, 见下) */
    ULONGLONG rawId;        /* RawInput button_id (u64, Sorahk 兼容) */
    DWORD targetType;       /* TGT_* */
    DWORD targetKeys[MAP_TARGET_MAX];
    DWORD mouseId;          /* TGT_MOUSEBTN */
    DWORD moveDir;          /* TGT_MOUSE_MOVE: 1=上 2=上右 3=右 4=下右 5=下 6=下左 7=左 8=上左 */
    DWORD moveSpeed;        /* 移动速度(像素/帧) */
    DWORD scrollDir;        /* TGT_MOUSE_SCROLL */
    int   turbo;            /* 连发间隔 ms, 0=关闭 */
    int   duration;         /* 单次按键持续 ms */
    int   enabled;
    wchar_t note[40];
} VibMapEntry;

/* XInput 按钮 id (与 Sorahk xinput.rs 编号一致) */
enum {
    XID_A = 0x01, XID_B = 0x02, XID_X = 0x03, XID_Y = 0x04,
    XID_LB = 0x05, XID_RB = 0x06, XID_LS_CLICK = 0x07, XID_RS_CLICK = 0x08,
    XID_LT = 0x09, XID_RT = 0x0A, XID_DPAD_UP = 0x0B, XID_DPAD_DOWN = 0x0C,
    XID_DPAD_LEFT = 0x0D, XID_DPAD_RIGHT = 0x0E, XID_BACK = 0x0F, XID_START = 0x10,
    XID_LS_UP = 0x11, XID_LS_DOWN = 0x12, XID_LS_LEFT = 0x13, XID_LS_RIGHT = 0x14,
    XID_RS_UP = 0x15, XID_RS_DOWN = 0x16, XID_RS_LEFT = 0x17, XID_RS_RIGHT = 0x18,
};

/* 震动参数 */
typedef struct {
    int   enabled;
    int   autostart;
    int   attackGain;
    int   damageGain;
    int   shakeGain;
    int   moveGain;
    int   maxStrength;
    int   decayMs;
    int   hitBoost;
    int   startPulse;
    int   killPulse;
    int   killGain;          /* 怪物死亡震动 (VEV_KILL) */
    int   shakeScreenGain;   /* 镜头震动 [shake screen] (VEV_SHAKE_SCREEN) */
} VibSettings;

#define VIB_SETTINGS_DEFAULT { 1, 1, 40, 60, 80, 20, 100, 180, 15, 35, 50, 60, 60 }

/* ---------------- 旧版配置(兼容 DLL 与 ini 工具) ---------------- */
enum {
    VIB_PAD_X = 0, VIB_PAD_A, VIB_PAD_B, VIB_PAD_Y, VIB_PAD_LB, VIB_PAD_RB,
    VIB_PAD_LT, VIB_PAD_RT, VIB_PAD_BACK, VIB_PAD_START, VIB_PAD_LEFT,
    VIB_PAD_RIGHT, VIB_PAD_UP, VIB_PAD_DOWN, VIB_PAD_LS, VIB_PAD_RS
};

typedef struct {
    int   enabled;
    int   autostart;
    int   attackGain;
    int   damageGain;
    int   shakeGain;
    int   moveGain;
    int   maxStrength;
    int   decayMs;
    int   hitBoost;
    int   startPulse;
    int   killPulse;
    int   killGain;          /* 怪物死亡震动 (VEV_KILL) */
    int   shakeScreenGain;   /* 镜头震动 [shake screen] (VEV_SHAKE_SCREEN) */
    DWORD padMap[16];
} VibConfig;

#define VIB_CFG_DEFAULT { \
    1, 1, 40, 60, 80, 20, 100, 180, 15, 35, 50, 60, 60, \
    {0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0} }

/* 映射预设组 */
typedef struct {
    wchar_t name[64];
    int     count;
    VibMapEntry maps[MAP_MAX];
} VibPreset;

/* ---------------- DLL hook 地址(2026-07-23 build) ---------------- */
enum {
    HOOK_ON_ATTACK        = 0,
    HOOK_ON_DAMAGE        = 1,
    HOOK_ON_START_BATTLE  = 2,
    HOOK_ON_RESET_COMBO   = 3,
    HOOK_ON_ACTION_END    = 4,
    HOOK_ON_TARGET_DIE    = 5,
    HOOK_ON_SHAKE_INPUT   = 6,
    HOOK_COUNT            = 7
};

static const DWORD g_hookTargets[HOOK_COUNT] = {
    0x007885A0, 0x00788480, 0x00788730, 0x00788700,
    0x00788760, 0x007884C0, 0x02AFADB0,
};

#define DAMAGE_OBJ_VT_OFF   808
#define DAMAGE_VALUE_OFF    68

/* ---------------- 内存模式锚点 ---------------- */
#define MEM_G_DAMAGE_FONT   0x0415D2B4
#define MEM_G_AI_MANAGER    0x0415C5D8
#define MEM_CONTAINER_OFF   8
#define MEM_HEAD_OFF        4

/* ---------------- Sorahk 兼容常量 ---------------- */
#define RAW_SKIP_BYTES      5
#define RAW_MIN_DATA_SIZE   10
#define FNV32_OFFSET        0x811c9dc5u
#define FNV32_PRIME         0x01000193u
#define FNV64_OFFSET        0xcbf29ce484222325ull
#define FNV64_PRIME         0x100000001b3ull

static inline DWORD fnv1a_u32(DWORD h, DWORD v) { h ^= v; return h * FNV32_PRIME; }
static inline ULONGLONG fnv1a_u64(ULONGLONG h, ULONGLONG v) { h ^= v; return h * FNV64_PRIME; }

#endif /* VIB_PROTOCOL_H */
