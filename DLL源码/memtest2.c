/* ============================================================
 * DfoVibration-Mem2.dll — 字段扫描版 (评分 + 移速/攻速定位)
 * 1. obj+0x100~0x200 区间 (评分候选)
 * 2. n2500 人物对象前 0x4000 字节 (移速/攻速等属性)
 * 变化时记录 偏移+旧值+新值, 行为定位:
 *   - 走路 -> 变化的字段 = 移动速度相关
 *   - 攻击 -> 变化的字段 = 攻速相关
 *   - 打评分 -> obj+0x110 跳变 = 评分值
 * ============================================================ */
#include <windows.h>
#include <stdio.h>
#include <string.h>

#define BASE_MAIN   0x04189A88
#define FN_UNWRAP   0x1E7F410
#define BASE_N2500  0x04294CE8

#define SCAN_N2500  0x4000   /* 人物对象扫描 16KB */
#define WATCH_OBJ   0x100    /* 主对象 0x100-0x200 区间 */

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

/* 扫描区: 记录每个 dword, 变化时输出 */
typedef struct {
    DWORD base;
    DWORD size;
    DWORD *snap;
    int    armed;
    int    logCount;
} ScanCtx;

static void scan_init(ScanCtx *c, DWORD base, DWORD size)
{
    c->base = base;
    c->size = size;
    c->snap = (DWORD *)calloc(size / 4, sizeof(DWORD));
    c->armed = 0;
    c->logCount = 0;
}

static void scan_tick(ScanCtx *c, const char *tag)
{
    DWORD i;
    if (!c->base || !c->snap)
        return;
    for (i = 0; i < c->size / 4; i++) {
        DWORD v = try_read(c->base + i * 4);
        if (v != c->snap[i]) {
            if (c->armed) {
                dll_log("CHG %s +0x%04X: 0x%08X -> 0x%08X", tag,
                        i * 4, c->snap[i], v);
                c->logCount++;
                if (c->logCount > 300) {
                    dll_log("(scan limit reached for %s)", tag);
                    c->base = 0;
                    break;
                }
            }
            c->snap[i] = v;
        }
    }
}

static DWORD WINAPI scan_worker(LPVOID param)
{
    (void)param;
    ScanCtx ctx;
    DWORD lastObj = 0, lastN2500 = 0;
    DWORD obj = 0, n2500 = 0;
    int frames = 0;

    Sleep(2000);

    dll_log("========== mem scan v2 start ==========");
    dll_log("HINT: 走路观察 +0x?? 变化=移速; 攻击=攻速; 评分看 OBJ+0x110");

    scan_init(&ctx, 0, 0);

    while (g_running) {
        DWORD cache = try_read(BASE_MAIN);
        DWORD newObj = cache ? unwrap(cache) : 0;
        DWORD newN = try_read(BASE_N2500);

        /* 主对象或人物指针变化 -> 重建扫描区 */
        if (newObj != lastObj) {
            if (newObj)
                dll_log("OBJ -> 0x%08X", newObj);
            lastObj = newObj;
            obj = newObj;
        }
        if (newN != lastN2500) {
            if (newN)
                dll_log("N2500 -> 0x%08X", newN);
            lastN2500 = newN;
            n2500 = newN;
            if (n2500)
                scan_init(&ctx, n2500, SCAN_N2500);
        }

        if (obj) {
            /* 评分候选区固定轮询 (变化即记录, 含初值 0xFFFFFFFF) */
            DWORD v110 = try_read(obj + 0x110);
            static DWORD s110 = 0;
            if (v110 != s110) {
                dll_log("OBJ+0x110: 0x%08X -> 0x%08X", s110, v110);
                s110 = v110;
            }
            DWORD v10C = try_read(obj + 0x10C);
            static DWORD s10C = 0;
            if (v10C != s10C) {
                dll_log("OBJ+0x10C: 0x%08X -> 0x%08X", s10C, v10C);
                s10C = v10C;
            }
            DWORD v114 = try_read(obj + 0x114);
            static DWORD s114 = 0;
            if (v114 != s114) {
                dll_log("OBJ+0x114: 0x%08X -> 0x%08X", s114, v114);
                s114 = v114;
            }
        }

        /* 人物对象全字段扫描 */
        if (n2500)
            scan_tick(&ctx, "N2500");

        frames++;
        if (frames % 4 == 0) {
            /* 每 1 秒输出一次状态 (确认存活) */
            static int st = 0;
            if (st++ % 10 == 0)
                dll_log("STAT obj=0x%08X n2500=0x%08X", obj, n2500);
        }
        Sleep(250);
    }
    if (ctx.snap) free(ctx.snap);
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
