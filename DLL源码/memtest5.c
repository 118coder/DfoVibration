/* ============================================================
 * DfoVibration-Mem5.dll — 关键字段值监视版
 * 关注字段 (按之前分析):
 *   +0x3874            城镇移速候选
 *   +0x04C4~+0x059C    攻速/攻击状态区 (代表点)
 *   +0x030C +0x0470    攻击瞬间
 *   +0x23EC~+0x24D4    副本移动区 (代表点)
 *   +0x01B8 +0x0788 +0x0790  副本活动区
 * 每 500ms 输出全部关注字段的 hex + float 值
 * ============================================================ */
#include <windows.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>

#define BASE_MAIN   0x04189A88
#define FN_UNWRAP   0x1E7F410
#define BASE_N2500  0x04294CE8

#define MARK_DIR    "G:\\game\\USDOF\\us_extend_dll"

static char g_logPath[MAX_PATH] = "";
static volatile int g_running = 0;
static HANDLE g_hThread = NULL;
static DWORD g_tick0 = 0;

static void dll_log(const char *fmt, ...)
{
    FILE *fp;
    char buf[1024];
    va_list ap;
    if (!g_logPath[0]) return;
    va_start(ap, fmt);
    _vsnprintf(buf, sizeof(buf) - 1, fmt, ap);
    va_end(ap);
    fp = fopen(g_logPath, "a");
    if (fp) {
        fprintf(fp, "[%lu|+%ums] %s\n", (unsigned long)GetCurrentThreadId(),
                (unsigned long)(GetTickCount() - g_tick0), buf);
        fclose(fp);
    }
}

static void check_markers(void)
{
    static const char *names[] = {
        "MARK_WALK.txt", "MARK_ATTACK.txt", "MARK_SCORE.txt", "MARK_STOP.txt"
    };
    static const char *tags[] = {
        "==WALK START==", "==ATTACK START==", "==SCORE START==", "==STOP=="
    };
    char path[MAX_PATH];
    int i;
    for (i = 0; i < 4; i++) {
        _snprintf(path, sizeof(path), "%s\\%s", MARK_DIR, names[i]);
        if (GetFileAttributesA(path) != INVALID_FILE_ATTRIBUTES) {
            dll_log("%s", tags[i]);
            DeleteFileA(path);
        }
    }
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

/* 关注偏移表: 名称 + 偏移 */
static const struct { const char *name; DWORD off; } WATCH[] = {
    { "W3874", 0x3874 },   /* 城镇移速候选 */
    { "W3878", 0x3878 },
    { "W3888", 0x3888 },
    { "A04C4", 0x04C4 },   /* 攻速区代表 */
    { "A04CC", 0x04CC },
    { "A0504", 0x0504 },
    { "A0544", 0x0544 },
    { "A0584", 0x0584 },
    { "A0594", 0x0594 },
    { "A030C", 0x030C },   /* 攻击瞬间 */
    { "A0470", 0x0470 },
    { "A0310", 0x0310 },
    { "D23EC", 0x23EC },   /* 副本移动区 */
    { "D2404", 0x2404 },
    { "D2444", 0x2444 },
    { "D2494", 0x2494 },
    { "D24D4", 0x24D4 },
    { "D01B8", 0x01B8 },   /* 副本活动区 */
    { "D01BC", 0x01BC },
    { "D0788", 0x0788 },
    { "D0790", 0x0790 },
    { "D0794", 0x0794 },
    { "D080C", 0x080C },
};
#define WATCH_N (sizeof(WATCH) / sizeof(WATCH[0]))

static DWORD WINAPI watch_worker(LPVOID param)
{
    (void)param;
    DWORD obj = 0, n2500 = 0;
    DWORD lastN = 0;
    DWORD tick = 0;

    g_tick0 = GetTickCount();
    Sleep(2000);
    dll_log("========== mem watch v5 start ==========");
    dll_log("HINT: 走路看 W3874/D23EC 值; 攻击看 A04C4 区; 评分看 OBJ+0x110");

    while (g_running) {
        check_markers();
        DWORD cache = try_read(BASE_MAIN);
        DWORD newObj = cache ? unwrap(cache) : 0;
        DWORD newN = try_read(BASE_N2500);

        if (newObj != obj) {
            if (newObj) dll_log("OBJ -> 0x%08X", newObj);
            obj = newObj;
        }
        if (newN != lastN) {
            if (newN) dll_log("N2500 -> 0x%08X", newN);
            lastN = newN;
            n2500 = newN;
        }

        /* OBJ+0x110 评分候选 */
        if (obj) {
            static DWORD s110 = 0, s110p = 0;
            DWORD v = try_read(obj + 0x110);
            if (v != s110) { dll_log("OBJ+0x110: 0x%08X -> 0x%08X", s110, v); s110 = v; }
            if (v != 0 && v != 0xFFFFFFFF) {
                DWORD vp = try_read(v);
                if (vp != s110p) { dll_log("  [OBJ+0x110] -> 0x%08X", vp); s110p = vp; }
            }
        }

        /* 关注字段值快照 (每 500ms) */
        if (n2500 && tick % 2 == 0) {
            char line[1024] = "";
            char tmp[96];
            DWORD i;
            for (i = 0; i < WATCH_N; i++) {
                DWORD v = try_read(n2500 + WATCH[i].off);
                float f = 0.0f;
                memcpy(&f, &v, 4);
                _snprintf(tmp, sizeof(tmp), "%s%s=0x%08X(%.1f)", i ? " " : "",
                          WATCH[i].name, v, f);
                if (strlen(line) + strlen(tmp) < sizeof(line) - 2)
                    strcat(line, tmp);
            }
            dll_log("VAL %s", line);
        }

        tick++;
        Sleep(250);
    }
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
        g_hThread = CreateThread(NULL, 0, watch_worker, NULL, 0, NULL);
    } else if (reason == DLL_PROCESS_DETACH) {
        g_running = 0;
        if (g_hThread) { WaitForSingleObject(g_hThread, 1000); g_hThread = NULL; }
    }
    return TRUE;
}
