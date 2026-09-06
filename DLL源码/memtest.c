/* ============================================================
 * DfoVibration-Mem.dll — 内存基址验证测试 DLL
 * 用途: 验证 [[0x4189A88]+0x110] 是否为评分系统值
 * 原理: 轮询读取游戏内存, 值变化时写日志 (无需 CE)
 * 观察: 进副本打怪, 看日志中 0x110 值是否随评分变化
 * ============================================================ */
#include <windows.h>
#include <stdio.h>
#include <string.h>

#define BASE_MAIN   0x04189A88   /* 主对象缓存句柄 */
#define FN_UNWRAP   0x1E7F410    /* 解引用: thiscall(cache)->main obj */
#define BASE_N2500  0x04294CE8   /* 人物属性 */

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

/* 读 [addr] 若已提交 */
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

/* thiscall 调用游戏函数 sub_1E7F410(cache) -> main obj */
typedef DWORD (__attribute__((thiscall)) *FnUnwrap)(DWORD thisptr);
static DWORD unwrap(DWORD cache)
{
    FnUnwrap fn = (FnUnwrap)FN_UNWRAP;
    return fn(cache);
}

static DWORD WINAPI mem_worker(LPVOID param)
{
    (void)param;
    DWORD lastMain = 0, lastObj = 0, last110 = 0;
    DWORD lastN2500 = 0;
    DWORD lastN110 = 0;
    int logCount = 0;

    Sleep(2000);   /* 等游戏初始化完成 */

    dll_log("========== mem verify start (base=0x%08X unwrap=0x%08X n2500=0x%08X) ==========",
            BASE_MAIN, FN_UNWRAP, BASE_N2500);

    while (g_running) {
        DWORD cache = try_read(BASE_MAIN);
        DWORD obj = 0;
        DWORD v110 = 0, v104 = 0, v10C = 0, v114 = 0, v11C = 0;
        DWORD n2500 = try_read(BASE_N2500);
        DWORD n110 = 0;

        if (cache)
            obj = unwrap(cache);
        if (obj) {
            v104 = try_read(obj + 0x104);
            v10C = try_read(obj + 0x10C);
            v110 = try_read(obj + 0x110);
            v114 = try_read(obj + 0x114);
            v11C = try_read(obj + 0x11C);
        }
        if (n2500)
            n110 = try_read(n2500 + 0x110);

        /* 只记录变化, 避免刷屏 */
        if (cache != lastMain || obj != lastObj || v110 != last110 ||
            n2500 != lastN2500 || n110 != lastN110 || logCount == 0) {
            dll_log("MEM cache=0x%08X obj=0x%08X [+110]=0x%08X (+104=0x%08X +10C=0x%08X +114=0x%08X +11C=0x%08X) n2500=0x%08X n[+110]=0x%08X",
                    cache, obj, v110, v104, v10C, v114, v11C, n2500, n110);
            lastMain = cache;
            lastObj = obj;
            last110 = v110;
            lastN2500 = n2500;
            lastN110 = n110;
            logCount++;
            if (logCount > 200) {
                dll_log("(log limit reached)");
                break;
            }
        }
        Sleep(250);
    }
    return 0;
}

BOOL WINAPI DllMain(HINSTANCE hDll, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        HMODULE self = hDll;
        /* 路径: 与 DLL 同目录, 名 = DLL名去掉.dll + _mem.log */
        GetModuleFileNameA(self, g_logPath, MAX_PATH);
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
        g_hThread = CreateThread(NULL, 0, mem_worker, NULL, 0, NULL);
    } else if (reason == DLL_PROCESS_DETACH) {
        g_running = 0;
        if (g_hThread) { WaitForSingleObject(g_hThread, 1000); g_hThread = NULL; }
    }
    return TRUE;
}
