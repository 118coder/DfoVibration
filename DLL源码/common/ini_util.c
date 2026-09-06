/* 简易 INI 读写: ANSI/UTF-8 均可(注释 ; #), 无第三方依赖 */
#include <windows.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "ini_util.h"
#include "vib_protocol.h"

static char *trim(char *s)
{
    char *e;
    while (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n') s++;
    e = s + strlen(s);
    while (e > s && (e[-1] == ' ' || e[-1] == '\t' || e[-1] == '\r' || e[-1] == '\n')) *--e = 0;
    return s;
}

int ini_read_int(const char *path, const char *section, const char *key, int def)
{
    FILE *fp = fopen(path, "rb");
    char line[512];
    char cur[128] = "";
    int  result = def;

    if (!fp) return def;
    while (fgets(line, sizeof(line), fp)) {
        char buf[512];
        char *p, *eq;
        strncpy(buf, line, sizeof(buf) - 1);
        buf[sizeof(buf) - 1] = 0;
        p = trim(buf);
        if (*p == ';' || *p == '#' || *p == 0) continue;
        if (*p == '[') {
            char *e = strchr(p, ']');
            if (e) { *e = 0; strncpy(cur, trim(p + 1), sizeof(cur) - 1); }
            continue;
        }
        if (!_stricmp(cur, section)) {
            eq = strchr(p, '=');
            if (eq) {
                *eq = 0;
                if (!_stricmp(trim(p), key)) {
                    result = atoi(trim(eq + 1));
                    break;
                }
            }
        }
    }
    fclose(fp);
    return result;
}

void ini_write_int(const char *path, const char *section, const char *key, int val)
{
    FILE *fp = fopen(path, "r+b");
    /* 简化: 追加节与键值(重复时保留旧值, 由 save_config 全量重写) */
    if (!fp) fp = fopen(path, "ab");
    if (!fp) return;
    fseek(fp, 0, SEEK_END);
    fprintf(fp, "\n[%s]\n%s=%d\n", section, key, val);
    fclose(fp);
}

/* 全量重写配置(带注释头) */
void ini_save_config(const char *path, const VibConfig *cfg)
{
    FILE *fp = fopen(path, "wb");
    if (!fp) return;
    fprintf(fp,
        "; DfoVibration.ini - DFO Vibration Plugin config (same dir as DLL)\r\n"
        "; Enabled      - master switch: 1=on(default) 0=off\r\n"
        "; AutoStartExe - auto open control panel after DLL mounted: 1=on 0=off\r\n"
        "; AttackGain   - attack frequency gain 0-100\r\n"
        "; DamageGain   - hit damage gain 0-100\r\n"
        "; ShakeGain    - camera shake gain 0-100\r\n"
        "; MoveGain     - movement gain 0-100\r\n"
        "; MaxStrength  - total strength cap 0-100\r\n"
        "; DecayMs      - vibration decay time constant (ms)\r\n"
        "; HitBoost     - combo reset instant boost 0-100\r\n"
        "; StartPulse   - battle start pulse 0-100\r\n"
        "; KillPulse    - kill pulse 0-100\r\n"
        "; [PadMap] gamepad button -> virtual key code (0=disabled), see panel help\r\n"
        "\r\n[Main]\r\nEnabled=%d\r\nAutoStartExe=%d\r\n"
        "\r\n[Vibration]\r\n"
        "AttackGain=%d\r\nDamageGain=%d\r\nShakeGain=%d\r\nMoveGain=%d\r\n"
        "MaxStrength=%d\r\nDecayMs=%d\r\nHitBoost=%d\r\nStartPulse=%d\r\nKillPulse=%d\r\n"
        "\r\n[PadMap]\r\n"
        "X=%u\r\nA=%u\r\nB=%u\r\nY=%u\r\nLB=%u\r\nRB=%u\r\nLT=%u\r\nRT=%u\r\n"
        "Back=%u\r\nStart=%u\r\nLeft=%u\r\nRight=%u\r\nUp=%u\r\nDown=%u\r\nLS=%u\r\nRS=%u\r\n",
        cfg->enabled ? 1 : 0, cfg->autostart ? 1 : 0,
        cfg->attackGain, cfg->damageGain, cfg->shakeGain, cfg->moveGain,
        cfg->maxStrength, cfg->decayMs, cfg->hitBoost, cfg->startPulse, cfg->killPulse,
        (unsigned int)cfg->padMap[VIB_PAD_X], (unsigned int)cfg->padMap[VIB_PAD_A],
        (unsigned int)cfg->padMap[VIB_PAD_B],
        (unsigned int)cfg->padMap[VIB_PAD_Y], (unsigned int)cfg->padMap[VIB_PAD_LB],
        (unsigned int)cfg->padMap[VIB_PAD_RB],
        (unsigned int)cfg->padMap[VIB_PAD_LT], (unsigned int)cfg->padMap[VIB_PAD_RT],
        (unsigned int)cfg->padMap[VIB_PAD_BACK],
        (unsigned int)cfg->padMap[VIB_PAD_START], (unsigned int)cfg->padMap[VIB_PAD_LEFT],
        (unsigned int)cfg->padMap[VIB_PAD_RIGHT],
        (unsigned int)cfg->padMap[VIB_PAD_UP], (unsigned int)cfg->padMap[VIB_PAD_DOWN],
        (unsigned int)cfg->padMap[VIB_PAD_LS],
        (unsigned int)cfg->padMap[VIB_PAD_RS]);
    fclose(fp);
}

int ini_load_config(const char *path, VibConfig *cfg)
{
    VibConfig d = VIB_CFG_DEFAULT;
    *cfg = d;
    cfg->enabled       = ini_read_int(path, "Main", "Enabled", d.enabled);
    cfg->autostart     = ini_read_int(path, "Main", "AutoStartExe", d.autostart);
    cfg->attackGain    = ini_read_int(path, "Vibration", "AttackGain", d.attackGain);
    cfg->damageGain    = ini_read_int(path, "Vibration", "DamageGain", d.damageGain);
    cfg->shakeGain     = ini_read_int(path, "Vibration", "ShakeGain", d.shakeGain);
    cfg->moveGain      = ini_read_int(path, "Vibration", "MoveGain", d.moveGain);
    cfg->maxStrength   = ini_read_int(path, "Vibration", "MaxStrength", d.maxStrength);
    cfg->decayMs       = ini_read_int(path, "Vibration", "DecayMs", d.decayMs);
    cfg->hitBoost      = ini_read_int(path, "Vibration", "HitBoost", d.hitBoost);
    cfg->startPulse    = ini_read_int(path, "Vibration", "StartPulse", d.startPulse);
    cfg->killPulse     = ini_read_int(path, "Vibration", "KillPulse", d.killPulse);
    cfg->padMap[VIB_PAD_X]     = (DWORD)ini_read_int(path, "PadMap", "X", 0);
    cfg->padMap[VIB_PAD_A]     = (DWORD)ini_read_int(path, "PadMap", "A", 0);
    cfg->padMap[VIB_PAD_B]     = (DWORD)ini_read_int(path, "PadMap", "B", 0);
    cfg->padMap[VIB_PAD_Y]     = (DWORD)ini_read_int(path, "PadMap", "Y", 0);
    cfg->padMap[VIB_PAD_LB]    = (DWORD)ini_read_int(path, "PadMap", "LB", 0);
    cfg->padMap[VIB_PAD_RB]    = (DWORD)ini_read_int(path, "PadMap", "RB", 0);
    cfg->padMap[VIB_PAD_LT]    = (DWORD)ini_read_int(path, "PadMap", "LT", 0);
    cfg->padMap[VIB_PAD_RT]    = (DWORD)ini_read_int(path, "PadMap", "RT", 0);
    cfg->padMap[VIB_PAD_BACK]  = (DWORD)ini_read_int(path, "PadMap", "Back", 0);
    cfg->padMap[VIB_PAD_START] = (DWORD)ini_read_int(path, "PadMap", "Start", 0);
    cfg->padMap[VIB_PAD_LEFT]  = (DWORD)ini_read_int(path, "PadMap", "Left", 0);
    cfg->padMap[VIB_PAD_RIGHT] = (DWORD)ini_read_int(path, "PadMap", "Right", 0);
    cfg->padMap[VIB_PAD_UP]    = (DWORD)ini_read_int(path, "PadMap", "Up", 0);
    cfg->padMap[VIB_PAD_DOWN]  = (DWORD)ini_read_int(path, "PadMap", "Down", 0);
    cfg->padMap[VIB_PAD_LS]    = (DWORD)ini_read_int(path, "PadMap", "LS", 0);
    cfg->padMap[VIB_PAD_RS]    = (DWORD)ini_read_int(path, "PadMap", "RS", 0);
    return 1;
}

/* ---------------- 预设 ---------------- */
int preset_save(const char *presetPath, const VibConfig *cfg)
{
    /* 预设只保存映射 + 震动参数(不保存主开关/AutoStart) */
    FILE *fp = fopen(presetPath, "wb");
    if (!fp) return 0;
    fprintf(fp,
        "; DFO Vibration Preset\r\n"
        "[Vibration]\r\n"
        "AttackGain=%d\r\nDamageGain=%d\r\nShakeGain=%d\r\nMoveGain=%d\r\n"
        "MaxStrength=%d\r\nDecayMs=%d\r\nHitBoost=%d\r\nStartPulse=%d\r\nKillPulse=%d\r\n"
        "\r\n[PadMap]\r\n"
        "X=%u\r\nA=%u\r\nB=%u\r\nY=%u\r\nLB=%u\r\nRB=%u\r\nLT=%u\r\nRT=%u\r\n"
        "Back=%u\r\nStart=%u\r\nLeft=%u\r\nRight=%u\r\nUp=%u\r\nDown=%u\r\nLS=%u\r\nRS=%u\r\n",
        cfg->attackGain, cfg->damageGain, cfg->shakeGain, cfg->moveGain,
        cfg->maxStrength, cfg->decayMs, cfg->hitBoost, cfg->startPulse, cfg->killPulse,
        (unsigned int)cfg->padMap[VIB_PAD_X], (unsigned int)cfg->padMap[VIB_PAD_A],
        (unsigned int)cfg->padMap[VIB_PAD_B],
        (unsigned int)cfg->padMap[VIB_PAD_Y], (unsigned int)cfg->padMap[VIB_PAD_LB],
        (unsigned int)cfg->padMap[VIB_PAD_RB],
        (unsigned int)cfg->padMap[VIB_PAD_LT], (unsigned int)cfg->padMap[VIB_PAD_RT],
        (unsigned int)cfg->padMap[VIB_PAD_BACK],
        (unsigned int)cfg->padMap[VIB_PAD_START], (unsigned int)cfg->padMap[VIB_PAD_LEFT],
        (unsigned int)cfg->padMap[VIB_PAD_RIGHT],
        (unsigned int)cfg->padMap[VIB_PAD_UP], (unsigned int)cfg->padMap[VIB_PAD_DOWN],
        (unsigned int)cfg->padMap[VIB_PAD_LS],
        (unsigned int)cfg->padMap[VIB_PAD_RS]);
    fclose(fp);
    return 1;
}

int preset_load(const char *presetPath, VibConfig *cfg)
{
    VibConfig t;
    if (!ini_load_config(presetPath, &t)) return 0;
    /* 覆盖主配置的映射与震动段, 保留 enabled/autostart */
    cfg->attackGain = t.attackGain; cfg->damageGain = t.damageGain;
    cfg->shakeGain = t.shakeGain;   cfg->moveGain = t.moveGain;
    cfg->maxStrength = t.maxStrength; cfg->decayMs = t.decayMs;
    cfg->hitBoost = t.hitBoost;     cfg->startPulse = t.startPulse;
    cfg->killPulse = t.killPulse;
    memcpy(cfg->padMap, t.padMap, sizeof(cfg->padMap));
    return 1;
}

int preset_delete(const char *presetPath)
{
    return DeleteFileA(presetPath) ? 1 : 0;
}

int preset_list(const char *dir, char names[][MAX_PATH], int max)
{
    WIN32_FIND_DATAA fd;
    HANDLE h;
    char pat[MAX_PATH * 2];
    int n = 0;
    _snprintf(pat, sizeof(pat), "%s\\*.ini", dir);
    h = FindFirstFileA(pat, &fd);
    if (h == INVALID_HANDLE_VALUE) return 0;
    do {
        if (!(fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) && n < max) {
            strncpy(names[n], fd.cFileName, MAX_PATH - 1);
            names[n][MAX_PATH - 1] = 0;
            n++;
        }
    } while (FindNextFileA(h, &fd));
    FindClose(h);
    return n;
}

int find_config_path(const char *selfDir, char *out, int outLen)
{
    char p[MAX_PATH * 2];
    /* 1) 本目录 */
    _snprintf(p, sizeof(p), "%s\\%s", selfDir, VIB_INI_FILE);
    if (GetFileAttributesA(p) != INVALID_FILE_ATTRIBUTES) {
        strncpy(out, p, outLen - 1); out[outLen - 1] = 0;
        return 1;
    }
    /* 2) 本目录\us_extend_dll\ (DLL 官方挂载目录) */
    _snprintf(p, sizeof(p), "%s\\us_extend_dll\\%s", selfDir, VIB_INI_FILE);
    if (GetFileAttributesA(p) != INVALID_FILE_ATTRIBUTES) {
        strncpy(out, p, outLen - 1); out[outLen - 1] = 0;
        return 1;
    }
    return 0;
}
