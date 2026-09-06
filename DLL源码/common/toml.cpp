/* TOML 子集解析器(线性扫描版) + 写出器 */
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "toml.h"

/* ---------------- 内部: 文本行索引 ---------------- */
typedef struct {
    int   start;       /* 行起点(含) */
    int   end;         /* 行终点(不含, -1=EOF) */
    char *text;        /* 原文本(未 strip) */
    char *value;       /* 去注释后 trim 的文本 */
    int   isSection;   /* 1=普通表 2=数组表 */
    char  path[TOML_MAX_PATH];
} TomlLine;

typedef struct {
    char   *data;      /* 整个文件 */
    int     len;
    TomlLine *lines;
    int     lineCount;
    int     capacity;
} TomlDoc;

static void toml_free_doc(TomlDoc *d)
{
    int i;
    if (d->lines) {
        for (i = 0; i < d->lineCount; i++)
            if (d->lines[i].value) free(d->lines[i].value);
        free(d->lines);
    }
    if (d->data) free(d->data);
    memset(d, 0, sizeof(*d));
}

static char *trim(char *s)
{
    char *e;
    while (*s == ' ' || *s == '\t' || *s == '\r' || *s == '\n') s++;
    e = s + strlen(s);
    while (e > s && (e[-1] == ' ' || e[-1] == '\t' || e[-1] == '\r' || e[-1] == '\n')) *--e = 0;
    return s;
}

/* 去掉注释(引号外 # 或行首 ;) */
static void strip_comment(char *line)
{
    int inStr = 0;
    char *p;
    for (p = line; *p; p++) {
        if (*p == '"') inStr = !inStr;
        else if (*p == '#' && !inStr) { *p = 0; break; }
    }
}

/* 解析段头, 返回 1=普通表 2=数组表, path 输出 */
static int parse_section(char *t, char *path, int pathLen)
{
    char *p = t;
    int isArr = 0;
    if (p[0] == '[' && p[1] == '[') { isArr = 1; p += 2; }
    else if (p[0] == '[') p += 1;
    else return 0;
    {
        char *end = strchr(p, ']');
        if (!end) return 0;
        *end = 0;
        strncpy(path, trim(p), pathLen - 1);
        path[pathLen - 1] = 0;
        return isArr ? 2 : 1;
    }
}

/* 构建行索引 */
static int toml_index(TomlDoc *d)
{
    int i;
    char *p = d->data;
    int cap = 256;
    d->lines = (TomlLine *)malloc(sizeof(TomlLine) * cap);
    d->lineCount = 0;

    for (i = 0; p && *p;) {
        char *nl = strchr(p, '\n');
        TomlLine *l;
        int end = nl ? (int)(nl - p) : (int)strlen(p);
        if (d->lineCount >= cap) {
            cap *= 2;
            d->lines = (TomlLine *)realloc(d->lines, sizeof(TomlLine) * cap);
        }
        l = &d->lines[d->lineCount++];
        memset(l, 0, sizeof(*l));
        l->start = i;
        l->end = nl ? i + end : -1;
        l->text = p;
        /* 拷贝一行做处理 */
        {
            char buf[2048];
            int n = end < 2047 ? end : 2047;
            memcpy(buf, p, n);
            buf[n] = 0;
            strip_comment(buf);
            l->value = _strdup(trim(buf));   /* 持久分配, 防止悬垂 */
            l->isSection = parse_section(l->value, l->path, TOML_MAX_PATH);
        }
        p = nl ? nl + 1 : NULL;
        i = l->end + 1;
    }
    return d->lineCount;
}

/* 段范围: 第 n(0-based) 个 path 段; outStart/outEnd = 行索引 */
static int toml_section_range(TomlDoc *d, const char *path, int n, int *outStart, int *outEnd)
{
    int i, cnt = 0;
    int found = -1;
    for (i = 0; i < d->lineCount; i++) {
        if (d->lines[i].isSection && !strcmp(d->lines[i].path, path)) {
            if (cnt == n) { found = i; break; }
            cnt++;
        }
    }
    if (found < 0) return 0;
    *outStart = found;
    *outEnd = d->lineCount;   /* 默认到文件尾 */
    /* 找下一个"同层或更浅"段结束 */
    for (i = found + 1; i < d->lineCount; i++) {
        if (d->lines[i].isSection) {
            /* 同层数组表或顶层段 -> 结束 */
            if (d->lines[i].isSection == 2) {
                /* 同 path 或父级 */
                if (!strcmp(d->lines[i].path, path)) { *outEnd = i; break; }
            } else if (d->lines[i].isSection == 1) {
                *outEnd = i; break;
            }
        }
    }
    return 1;
}

/* 在 [start,end) 行内找 key = value 行, 返回 value 字符串(静态) */
static const char *toml_find_kv(TomlDoc *d, int start, int end, const char *key)
{
    int i;
    char kbuf[128];
    for (i = start; i < end && i < d->lineCount; i++) {
        TomlLine *l = &d->lines[i];
        char *eq;
        if (l->isSection) continue;
        if (!l->value || !*l->value) continue;
        eq = strchr(l->value, '=');
        if (!eq) continue;
        {
            int n = (int)(eq - l->value);
            if (n >= (int)sizeof(kbuf)) n = sizeof(kbuf) - 1;
            memcpy(kbuf, l->value, n);
            kbuf[n] = 0;
            if (!strcmp(trim(kbuf), key))
                return trim(eq + 1);
        }
    }
    return NULL;
}

/* 解析 TOML 文档 */
static int toml_load(TomlDoc *d, const char *path)
{
    FILE *fp = fopen(path, "rb");
    long sz;
    if (!fp) return 0;
    fseek(fp, 0, SEEK_END);
    sz = ftell(fp);
    fseek(fp, 0, SEEK_SET);
    if (sz <= 0 || sz > 4 * 1024 * 1024) { fclose(fp); return 0; }
    d->data = (char *)malloc((size_t)sz + 1);
    if (!d->data) { fclose(fp); return 0; }
    if (fread(d->data, 1, (size_t)sz, fp) != (size_t)sz) {
        fclose(fp);
        toml_free_doc(d);
        return 0;
    }
    d->data[sz] = 0;
    fclose(fp);
    /* 去 BOM */
    if ((unsigned char)d->data[0] == 0xEF && (unsigned char)d->data[1] == 0xBB &&
        (unsigned char)d->data[2] == 0xBF) {
        memmove(d->data, d->data + 3, (size_t)sz - 2);
    }
    d->len = (int)strlen(d->data);
    toml_index(d);
    return d->lineCount > 0;
}

/* 解析路径: "presets[i].mappings[j].key" */
static int parse_path(const char *path, char *base, int *arrIdx, int *arrIdx2)
{
    /* 支持两种: "presets[0].mappings[1].key" 或 "presets.mappings.key" */
    const char *b = path, *p = path;
    char buf[TOML_MAX_PATH];
    int hasArr = 0;
    *arrIdx = 0; *arrIdx2 = -1;
    /* 段1 */
    while (*p && *p != '[' && *p != '.') p++;
    {
        int n = (int)(p - b);
        if (n >= TOML_MAX_PATH) n = TOML_MAX_PATH - 1;
        memcpy(buf, b, n);
        buf[n] = 0;
    }
    if (*p == '[') {
        hasArr = 1;
        *arrIdx = atoi(p + 1);
        p = strchr(p, ']');
        if (p) p++;
    }
    if (*p == '.') p++;
    /* 段2(可选) */
    {
        const char *b2 = p;
        while (*p && *p != '[' && *p != '.') p++;
        {
            int n = (int)(p - b2);
            if (n >= TOML_MAX_PATH) n = TOML_MAX_PATH - 1;
            memcpy(buf, b2, n);
            buf[n] = 0;
        }
        if (*p == '[') {
            *arrIdx2 = atoi(p + 1);
            p = strchr(p, ']');
            if (p) p++;
        }
        if (*p == '.') p++;
    }
    strncpy(base, buf, TOML_MAX_PATH - 1);
    base[TOML_MAX_PATH - 1] = 0;
    return hasArr;
}

/* 定位到目标段并返回其行范围(嵌套路径通用, 如 "presets[0].mappings[1]") */
static int toml_locate(TomlDoc *d, const char *path, int *outStart, int *outEnd)
{
    char seg[2][TOML_MAX_PATH];
    int idx[2] = { 0, 0 };
    const char *p = path;
    int segCount = 0;

    /* 分解段: seg0[.seg1], 每段可带 [n] */
    {
        char buf[TOML_MAX_PATH];
        const char *b = p;
        while (*p && *p != '.' && *p != '[') p++;
        {
            int n = (int)(p - b);
            if (n >= TOML_MAX_PATH) n = TOML_MAX_PATH - 1;
            memcpy(buf, b, n);
            buf[n] = 0;
            strncpy(seg[0], buf, TOML_MAX_PATH - 1);
            seg[0][TOML_MAX_PATH - 1] = 0;
        }
        if (*p == '[') { idx[0] = atoi(p + 1); p = strchr(p, ']'); if (p) p++; }
        segCount = 1;
        if (*p == '.') {
            p++;
            b = p;
            while (*p && *p != '.' && *p != '[') p++;
            {
                int n = (int)(p - b);
                if (n >= TOML_MAX_PATH) n = TOML_MAX_PATH - 1;
                memcpy(buf, b, n);
                buf[n] = 0;
                strncpy(seg[1], buf, TOML_MAX_PATH - 1);
                seg[1][TOML_MAX_PATH - 1] = 0;
            }
            if (*p == '[') { idx[1] = atoi(p + 1); p = strchr(p, ']'); if (p) p++; }
            segCount = 2;
        }
    }

    if (segCount == 1) {
        return toml_section_range(d, seg[0], idx[0], outStart, outEnd);
    }
    /* 两层: 先定位 seg0 段, 在其中定位 seg1 子段(完整路径 seg0.seg1) */
    {
        int s0, e0;
        int i, cnt = 0, found = -1;
        char full[TOML_MAX_PATH * 2];
        _snprintf(full, sizeof(full), "%s.%s", seg[0], seg[1]);
        if (!toml_section_range(d, seg[0], idx[0], &s0, &e0)) return 0;
        for (i = s0; i < e0; i++) {
            TomlLine *l = &d->lines[i];
            if (l->isSection && !strcmp(l->path, full)) {
                if (cnt == idx[1]) { found = i; break; }
                cnt++;
            }
        }
        if (found < 0) return 0;
        *outStart = found;
        *outEnd = e0;
        for (i = found + 1; i < e0; i++) {
            if (d->lines[i].isSection == 2 && !strcmp(d->lines[i].path, full)) {
                *outEnd = i;
                break;
            }
        }
        return 1;
    }
}

/* ---------------- 公共查询 ---------------- */
static const char *toml_get_kv(const char *path, const char *tomlPath, const char *key)
{
    static TomlDoc doc;
    static int loaded = 0;
    const char *kv;
    int s, e;
    if (!loaded) {
        if (!toml_load(&doc, tomlPath)) return NULL;
        loaded = 1;
    }
    if (!toml_locate(&doc, path, &s, &e)) return NULL;
    kv = toml_find_kv(&doc, s, e, key);
    return kv;
}

/* 从"字符串值"提取引号内内容 */
static int parse_str(const char *v, char *out, int outLen)
{
    const char *p;
    if (!v) return 0;
    p = v;
    while (*p == ' ') p++;
    if (*p == '"') {
        p++;
        {
            int n = 0;
            while (*p && *p != '"' && n < outLen - 1) out[n++] = *p++;
            out[n] = 0;
            return 1;
        }
    }
    return 0;
}

int toml_get_str(const char *text, const char *path, const char *key, char *out, int outLen)
{
    const char *v;
    int s, e;
    static TomlDoc doc;
    static int loaded = 0;
    if (!loaded) {
        if (!toml_load(&doc, text)) return 0;
        loaded = 1;
    }
    if (!toml_locate(&doc, path, &s, &e)) return 0;
    v = toml_find_kv(&doc, s, e, key);
    return parse_str(v, out, outLen);
}

/* 通用文本入口(路径=段路径, key 单独传) */
static int toml_ctx(const char *text, const char *path, const char *key,
                    char *strOut, int strLen, int *intOut, int *boolOut)
{
    static TomlDoc doc;
    static int loaded = 0;
    const char *v;
    int s, e;
    if (!loaded) {
        if (!toml_load(&doc, text)) return 0;
        loaded = 1;
    }
    if (!toml_locate(&doc, path, &s, &e)) return 0;
    v = toml_find_kv(&doc, s, e, key);
    if (!v) return 0;
    if (strOut && parse_str(v, strOut, strLen)) return 1;
    if (intOut) { *intOut = atoi(v); return 2; }
    if (boolOut) {
        *boolOut = !_stricmp(v, "true") || !strcmp(v, "1");
        return 3;
    }
    return 0;
}

int toml_get_int(const char *text, const char *path, const char *key, int def)
{
    int v = def;
    int r = toml_ctx(text, path, key, NULL, 0, &v, NULL);
    if (r == 2) return v;
    if (r == 3) return v;   /* bool as int */
    return def;
}

int toml_get_bool(const char *text, const char *path, const char *key, int def)
{
    int v = def;
    int r = toml_ctx(text, path, key, NULL, 0, NULL, &v);
    return r == 3 ? v : def;
}

void toml_foreach_str(const char *text, const char *path, const char *key,
                      TomlStrArrayCb cb, void *user)
{
    static TomlDoc doc;
    static int loaded = 0;
    const char *v;
    int s, e;
    if (!loaded) {
        if (!toml_load(&doc, text)) return;
        loaded = 1;
    }
    if (!toml_locate(&doc, path, &s, &e)) return;
    v = toml_find_kv(&doc, s, e, key);
    if (!v) return;
    while (*v == ' ') v++;
    if (*v == '[') {
        v++;
        for (;;) {
            while (*v == ' ' || *v == ',' || *v == '\r' || *v == '\n') v++;
            if (*v == ']' || !*v) break;
            if (*v == '"') {
                char buf[128];
                int n = 0;
                v++;
                while (*v && *v != '"' && n < 127) buf[n++] = *v++;
                buf[n] = 0;
                if (*v == '"') v++;
                cb(buf, user);
            } else {
                /* 未加引号的 token */
                const char *b = v;
                while (*v && *v != ',' && *v != ']' && *v != ' ') v++;
                {
                    char buf[128];
                    int n = (int)(v - b);
                    if (n > 127) n = 127;
                    memcpy(buf, b, n);
                    buf[n] = 0;
                    cb(buf, user);
                }
            }
        }
    } else if (*v == '"') {
        char buf[128];
        if (parse_str(v, buf, sizeof(buf)))
            cb(buf, user);
    }
}

int toml_array_count(const char *text, const char *path)
{
    static TomlDoc doc;
    static int loaded = 0;
    int i, cnt = 0;
    int s = 0, e = 0;
    const char *dot = strrchr(path, '.');
    char full[TOML_MAX_PATH * 2];

    if (!loaded) {
        if (!toml_load(&doc, text)) return 0;
        loaded = 1;
    }

    if (!dot) {
        /* 一层: 全文件计数 */
        for (i = 0; i < doc.lineCount; i++)
            if (doc.lines[i].isSection == 2 && !strcmp(doc.lines[i].path, path))
                cnt++;
        return cnt;
    }

    /* 两层: 定位父段范围, 计数完整子路径(去掉 [n]) */
    {
        char parent[TOML_MAX_PATH];
        int n = (int)(dot - path);
        if (n >= TOML_MAX_PATH) n = TOML_MAX_PATH - 1;
        memcpy(parent, path, n);
        parent[n] = 0;
        if (!toml_locate(&doc, parent, &s, &e)) return 0;
        /* 子段完整路径: 去掉父索引后的 ".mappings" 部分 */
        {
            const char *sub = dot + 1;
            char subSeg[TOML_MAX_PATH];
            const char *b = sub;
            while (*sub && *sub != '[' && *sub != '.') sub++;
            {
                int m = (int)(sub - b);
                if (m >= TOML_MAX_PATH) m = TOML_MAX_PATH - 1;
                memcpy(subSeg, b, m);
                subSeg[m] = 0;
                {
                    char pseg[TOML_MAX_PATH];
                    const char *b2 = parent;
                    while (*b2 && *b2 != '[' && *b2 != '.') b2++;
                    {
                        int m2 = (int)(b2 - parent);
                        if (m2 >= TOML_MAX_PATH) m2 = TOML_MAX_PATH - 1;
                        memcpy(pseg, parent, m2);
                        pseg[m2] = 0;
                        _snprintf(full, sizeof(full), "%s.%s", pseg, subSeg);
                    }
                }
            }
        }
        for (i = s; i < e; i++)
            if (doc.lines[i].isSection == 2 && !strcmp(doc.lines[i].path, full))
                cnt++;
        return cnt;
    }
}

/* ---------------- 写出 ---------------- */
static const char *key_name(int vk)
{
    /* 键盘键名(ASCII 键用单字符, 特殊键用名称) */
    switch (vk) {
    case 0x20: return "SPACE";
    case 0x0D: return "RETURN";
    case 0x1B: return "ESCAPE";
    case 0x09: return "TAB";
    case 0x08: return "BACKSPACE";
    case 0x2E: return "DELETE";
    case 0x2D: return "INSERT";
    case 0x26: return "UP";
    case 0x28: return "DOWN";
    case 0x25: return "LEFT";
    case 0x27: return "RIGHT";
    case 0x21: return "PAGEUP";
    case 0x22: return "PAGEDOWN";
    case 0x24: return "HOME";
    case 0x23: return "END";
    case 0x10: return "LSHIFT";
    case 0x11: return "LCTRL";
    case 0x12: return "LALT";
    case 0x5B: return "LWIN";
    case 0x5C: return "RWIN";
    default:
        if (vk >= 'A' && vk <= 'Z') {
            static char b[2];
            b[0] = (char)vk; b[1] = 0;
            return b;
        }
        if (vk >= '0' && vk <= '9') {
            static char b[2];
            b[0] = (char)vk; b[1] = 0;
            return b;
        }
        if (vk >= 0x70 && vk <= 0x87) {
            static char b[8];
            _snprintf(b, sizeof(b), "F%d", vk - 0x70 + 1);
            return b;
        }
        return "?";
    }
}

static const char *xid_name(DWORD id)
{
    switch (id) {
    case XID_A: return "A";
    case XID_B: return "B";
    case XID_X: return "X";
    case XID_Y: return "Y";
    case XID_LB: return "LB";
    case XID_RB: return "RB";
    case XID_LS_CLICK: return "LS_Click";
    case XID_RS_CLICK: return "RS_Click";
    case XID_LT: return "LT";
    case XID_RT: return "RT";
    case XID_DPAD_UP: return "DPad_Up";
    case XID_DPAD_DOWN: return "DPad_Down";
    case XID_DPAD_LEFT: return "DPad_Left";
    case XID_DPAD_RIGHT: return "DPad_Right";
    case XID_BACK: return "Back";
    case XID_START: return "Start";
    case XID_LS_UP: return "LS_Up";
    case XID_LS_DOWN: return "LS_Down";
    case XID_LS_LEFT: return "LS_Left";
    case XID_LS_RIGHT: return "LS_Right";
    case XID_RS_UP: return "RS_Up";
    case XID_RS_DOWN: return "RS_Down";
    case XID_RS_LEFT: return "RS_Left";
    case XID_RS_RIGHT: return "RS_Right";
    default: return "?";
    }
}

static void write_trigger(FILE *fp, const VibMapEntry *m)
{
    if (m->src == TRIG_XINPUT) {
        fprintf(fp, "trigger_key = \"GAMEPAD_045E_%s\"\n", xid_name(m->xid));
    } else {
        ULONGLONG id = m->rawId;
        DWORD stable = (DWORD)(id >> 32);
        DWORD pos = (DWORD)(id & 0xFFFFFFFF);
        fprintf(fp, "trigger_key = \"GAMEPAD_045E_02E0_DEV%08X_B%u.%u\"\n",
                stable, (unsigned)(pos >> 16), (unsigned)(pos & 0xFFFF));
    }
}

static void write_targets(FILE *fp, const VibMapEntry *m)
{
    int i;
    fprintf(fp, "target_keys = [");
    for (i = 0; i < MAP_TARGET_MAX; i++) {
        if (!m->targetKeys[i]) break;
        if (i) fprintf(fp, ", ");
        fprintf(fp, "\"%s\"", key_name(m->targetKeys[i]));
    }
    switch (m->targetType) {
    case TGT_MOUSEBTN:
        fprintf(fp, "%s\"%s\"", m->targetKeys[0] ? ", " : "",
                m->mouseId == 1 ? "LBUTTON" : m->mouseId == 2 ? "RBUTTON" :
                m->mouseId == 3 ? "MBUTTON" : "XBUTTON1");
        break;
    case TGT_MOUSE_MOVE:
        fprintf(fp, "%s\"%s\"", m->targetKeys[0] ? ", " : "",
                m->moveDir == 1 ? "MOUSE_UP" : m->moveDir == 3 ? "MOUSE_RIGHT" :
                m->moveDir == 5 ? "MOUSE_DOWN" : m->moveDir == 7 ? "MOUSE_LEFT" : "MOUSE_UP");
        break;
    case TGT_MOUSE_SCROLL:
        fprintf(fp, "%s\"%s\"", m->targetKeys[0] ? ", " : "",
                m->scrollDir == 1 ? "SCROLL_UP" : "SCROLL_DOWN");
        break;
    default:
        break;
    }
    fprintf(fp, "]\n");
}

void toml_write(const char *path, const VibSettings *st,
                const TomlPresetOut *presets, int presetCount)
{
    FILE *fp = fopen(path, "wb");
    int p, i;
    if (!fp) return;
    fprintf(fp, "# DfoVibration.toml - DFO Vibration Plugin v1.2 (mapping + settings)\n"
                "# Compatible with Sorahk preset format. Edit via GUI or text editor.\n\n");
    fprintf(fp, "[main]\n"
                "enabled = %s\n"
                "autostart_exe = %s\n\n",
                st->enabled ? "true" : "false",
                st->autostart ? "true" : "false");
    fprintf(fp, "[vibration]\n"
                "attack_gain = %d\n"
                "damage_gain = %d\n"
                "shake_gain = %d\n"
                "move_gain = %d\n"
                "max_strength = %d\n"
                "decay_ms = %d\n"
                "hit_boost = %d\n"
                "start_pulse = %d\n"
                "kill_pulse = %d\n"
                "kill_gain = %d\n"
                "shake_screen_gain = %d\n\n",
                st->attackGain, st->damageGain, st->shakeGain, st->moveGain,
                st->maxStrength, st->decayMs, st->hitBoost, st->startPulse,
                st->killPulse, st->killGain, st->shakeScreenGain);
    for (p = 0; p < presetCount; p++) {
        fprintf(fp, "[[presets]]\nname = \"%s\"\n", presets[p].name);
        for (i = 0; i < presets[p].count; i++) {
            const VibMapEntry *m = &presets[p].maps[i];
            fprintf(fp, "[[presets.mappings]]\n");
            write_trigger(fp, m);
            write_targets(fp, m);
            fprintf(fp, "interval = %d\n", m->turbo ? m->turbo : 5);
            fprintf(fp, "move_speed = %d\n", m->moveSpeed ? (int)m->moveSpeed : 5);
            fprintf(fp, "turbo_enabled = %s\n", m->turbo ? "true" : "false");
            fprintf(fp, "note = \"%ls\"\n", m->note);
            fprintf(fp, "\n");
        }
    }
    fclose(fp);
}
