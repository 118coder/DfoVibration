/* ============================================================
 * DfoVibration-Mem3.dll — 字段统计扫描版
 * 人物对象 16KB 按偏移统计变化次数, 每 2 秒输出汇总
 * 持续高频变化 = 位置/移动; 少量变化 = 速度/状态/攻速
 * OBJ+0x110/10C/114 评分候选: 变化即打印
 * ============================================================ */
#include <windows.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#define BASE_MAIN   0x04189A88
#define FN_UNWRAP   0x1E7F410
#define BASE_N2500  0x04294CE8

#define SCAN_SIZE   0x4000   /* 人物对象 16KB */

static char g_logPath[MAX_PATH] = "";
static volatile int g_running = 0;
static HANDLE g_hThread = NULL;

static void dll_log(const char *fmt, ...)
{
    FILE *fp;
    char buf[512];
    va_list ap;
    if (!g_logPath[0]) return;
    va_start(ap, fmt);
    _vsnprintf(buf, sizeof(buf) - 1, fmt, ap);
    va_end(ap);
    fp = fopen(g_logPath, "a");
    if (fp) { fprintf(fp, "[%lu] %s\n", (unsigned long)GetCurrentThreadId(), buf); fclose(fp); }
}

static DWORD try_read(DWORD addr)
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

typedef DWORD (__attribute__((thiscall)) *FnUnwrap)(DWORD thisptr);
static DWORD unwrap(DWORD cache)
{
    return ((FnUnwrap)FN_UNWRAP)(cache);
}

typedef struct {
    DWORD  base;
    DWORD  size;
    DWORD *snap;
    WORD  *chg;       /* 变化次数 */
    int    armed;     /* 快照建立后置 1 */
    DWORD  tickCount;
} ScanCtx;

static void scan_init(ScanCtx *c, DWORD base, DWORD size)
{
    if (c->snap) { free(c->snap); free(c->chg); }
    c->base = base;
    c->size = size;
    c->snap = (DWORD *)calloc(size / 4, sizeof(DWORD));
    c->chg  = (WORD *)calloc(size / 4, sizeof(WORD));
    c->armed = 0;
    c->tickCount = 0;
}

static void scan_tick(ScanCtx *c, const char *tag)
{
    DWORD i;
    if (!c->base || !c->snap || !c->chg)
        return;
    c->tickCount++;
    for (i = 0; i < c->size / 4; i++) {
        DWORD v = try_read(c->base + i * 4);
        if (v != c->snap[i]) {
            if (c->armed && c->chg[i] < 0xFFFF)
                c->chg[i]++;
            c->snap[i] = v;
        }
    }
    if (!c->armed) {
        c->armed = 1;   /* 首轮只建快照 */
        dll_log("SNAP %s base=0x%08X size=0x%X (基线已建立)", tag, c->base, c->size);
    }
    /* 每 8 tick (~2s) 输出一次统计 */
    if (c->armed && (c->tickCount % 8 == 0)) {
        DWORD n = 0;
        DWORD out[32];
        WORD  cnt[32];
        DWORD j;
        for (i = 0; i < c->size / 4 && n < 32; i++) {
            if (c->chg[i] > 0) {
                out[n] = i * 4;
                cnt[n] = c->chg[i];
                n++;
            }
        }
        if (n > 0) {
            char line[1024] = "";
            char tmp[64];
            for (j = 0; j < n; j++) {
                _snprintf(tmp, sizeof(tmp), "%s+0x%04X(x%u)", j ? " " : "", out[j], cnt[j]);
                if (strlen(line) + strlen(tmp) < sizeof(line) - 2)
                    strcat(line, tmp);
            }
            dll_log("CHGSTAT %s [%u]: %s", tag, c->tickCount / 8, line);
        }
        memset(c->chg, 0, c->size / 4 * sizeof(WORD));
    }
}

static void watch_obj(DWORD obj)
{
    static DWORD s110 = 0, s10C = 0, s114 = 0;
    DWORD v;
    v = try_read(obj + 0x110);
    if (v != s110) { dll_log("OBJ+0x110: 0x%08X -> 0x%08X", s110, v); s110 = v; }
    v = try_read(obj + 0x10C);
    if (v != s10C) { dll_log("OBJ+0x10C: 0x%08X -> 0x%08X", s10C, v); s10C = v; }
    v = try_read(obj + 0x114);
    if (v != s114) { dll_log("OBJ+0x114: 0x%08X -> 0x%08X", s114, v); s114 = v; }
}

static DWORD WINAPI scan_worker(LPVOID param)
{
    (void)param;
    ScanCtx ctx = {0};
    DWORD obj = 0, n2500 = 0;
    DWORD frames = 0;

    Sleep(2000);
    dll_log("========== mem scan v3 start ==========");
    dll_log("HINT: 站定3s->走路10s->停3s (城镇/副本各一轮); 攻速10s; 评分看 OBJ+0x110");

    while (g_running) {
        DWORD cache = try_read(BASE_MAIN);
        DWORD newObj = cache ? unwrap(cache) : 0;
        DWORD newN = try_read(BASE_N2500);

        if (newObj != obj) {
            if (newObj) dll_log("OBJ -> 0x%08X", newObj);
            obj = newObj;
        }
        if (newN != n2500) {
            if (newN) {
                dll_log("N2500 -> 0x%08X", newN);
                scan_init(&ctx, newN, SCAN_SIZE);
            } else {
                if (ctx.snap) { free(ctx.snap); free(ctx.chg); }
                memset(&ctx, 0, sizeof(ctx));
            }
            n2500 = newN;
        }

        if (obj) watch_obj(obj);
        if (n2500 && ctx.armed) scan_tick(&ctx, "N2500");
        else if (n2500 && !ctx.armed) scan_tick(&ctx, "N2500");

        frames++;
        if (frames % 40 == 0)   /* 每 10s */
            dll_log("STAT obj=0x%08X n2500=0x%08X armed=%d", obj, n2500, ctx.armed);

        Sleep(250);
    }
    if (ctx.snap) { free(ctx.snap); free(ctx.chg); }
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE hDll, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        GetModuleFileNameA(hDll, g_logPath, MAX_PATH);
        {
            char *slash = strrchr(g_logPath, '\\');
            if (slash) {
                char *dot = strrchr(slash + 1, '.');
                if (dot) *dot = 0;
                _snprintf(slash + 1, MAX_PATH - (slash - g_logPath) - 1,
                          "%s_mem.log", slash + 1);
            }
        }
        DisableThreadLibraryCalls(hDll);
        g_running = 1;
        g_hThread = CreateThread(NULL, 0, scan_worker, NULL, 0, NULL);
    } else if (reason == DLL_PROCESS_DETACH) {
        g_running = 0;
        if (g_hThread) { WaitForSingleObject(g_hThread, 1000); g_hThread = NULL; }
    }
    return TRUE;
}
