/* ============================================================
 * DFO 手柄震动插件 - 采集 DLL
 * 挂载: us_extend_dll\DfoVibration.dll (导出 DfoVibrationLoaded)
 * 或:   exe 注入 (LoadLibrary + VibPluginLoaded)
 * 功能: hook 行为事件 -> 写入共享内存环形缓冲
 * ============================================================ */
#include <windows.h>
#ifndef _WIN32_WINNT
#define _WIN32_WINNT 0x0501
#endif
#ifndef WINVER
#define WINVER 0x0501
#endif
#include <stdio.h>
#include <string.h>
#include <tlhelp32.h>
#include "hooks.h"
#include "../common/vib_protocol.h"
#include "../common/ini_util.h"

/* ---------------- 共享内存 ---------------- */
static HANDLE  g_hMap = NULL;
static VibShm *g_shm = NULL;
static volatile int g_running = 0;
static volatile int g_ringReady = 0;
static volatile int g_workerStarted = 0;
static HANDLE g_hWorker = NULL;
static HANDLE g_hCollector = NULL;
static char g_iniPath[MAX_PATH] = "";
static char g_logPath[MAX_PATH] = "";
static HMODULE g_hDllMod = NULL;

/* DLL 侧调试日志(与 DLL 同目录 DfoVibration_dll.log) */
void dll_log2(const char *fmt, ...)
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



/* 日志轮转: 超过 2MB 自动归档为 .old 并重新开始 */
static void log_rotate_check(void)
{
    HANDLE h;
    DWORD size;
    char oldPath[MAX_PATH];
    if (!g_logPath[0]) return;
    h = CreateFileA(g_logPath, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE,
                    NULL, OPEN_EXISTING, 0, NULL);
    if (h == INVALID_HANDLE_VALUE) return;
    size = GetFileSize(h, NULL);
    CloseHandle(h);
    if (size > 2 * 1024 * 1024) {
        _snprintf(oldPath, sizeof(oldPath), "%s.old", g_logPath);
        DeleteFileA(oldPath);
        MoveFileA(g_logPath, oldPath);
    }
}

/* 环形缓冲: CAS 保证多写者安全; 满判断用无符号差(回绕安全) */
static void ring_push(VibEvent *ev)
{
    VibRing *r;
    DWORD head, tail, pos, sz = sizeof(VibEvent), first;

    if (!g_shm) return;
    r = &g_shm->ring;
    for (;;) {
        head = r->head;
        tail = r->tail;
        /* 满: head 领先 tail 超过 capacity (无符号减, DWORD 回绕自动正确) */
        if ((DWORD)(head - tail) >= r->capacity)
            return;                       /* 满: 丢弃(不影响游戏) */
        pos = head % r->capacity;
        if (pos + sz <= r->capacity) {
            memcpy(r->data + pos, ev, sz);
        } else {
            first = r->capacity - pos;
            memcpy(r->data + pos, ev, first);
            memcpy(r->data, (BYTE *)ev + first, sz - first);
        }
        if (InterlockedCompareExchange((volatile LONG *)&r->head,
                                       (LONG)(head + sz), (LONG)head) == (LONG)head) {
            break;
        }
        /* 被并发写者抢先, 重试 */
    }
    InterlockedIncrement((volatile LONG *)&g_shm->seq);
    g_shm->lastTick = ev->tick;
}

/* 游戏线程回调(由 hooks.c 调用) */
static volatile DWORD g_cntAttack = 0;
static volatile DWORD g_cntDamage = 0;
static volatile DWORD g_cntShake = 0;

void vib_collect(int type, DWORD strength, DWORD tick, DWORD count)
{
    VibEvent ev;
    if (!g_ringReady) return;
    ev.type = (DWORD)type;
    ev.strength = strength;
    ev.tick = tick;
    ev.reserved = count;
    ring_push(&ev);
}

/* ---------------- 共享内存生命周期 ---------------- */
static int shm_open(void)
{
    if (g_shm) return 1;
    g_hMap = CreateFileMappingA(INVALID_HANDLE_VALUE, NULL, PAGE_READWRITE,
                                0, sizeof(VibShm), VIB_SHM_NAME);
    if (!g_hMap) {
        g_hMap = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, VIB_SHM_NAME);
        if (!g_hMap) return 0;
    }
    g_shm = (VibShm *)MapViewOfFile(g_hMap, FILE_MAP_ALL_ACCESS, 0, 0, sizeof(VibShm));
    if (!g_shm) {
        CloseHandle(g_hMap);
        g_hMap = NULL;
        return 0;
    }
    if (g_shm->magic != VIB_SHM_MAGIC) {
        memset(g_shm, 0, sizeof(VibShm));
        g_shm->magic = VIB_SHM_MAGIC;
        g_shm->version = VIB_SHM_VERSION;
        g_shm->ring.capacity = VIB_RING_SIZE;
    }
    g_shm->gamePid = GetCurrentProcessId();
    g_shm->flags |= 1;
    return 1;
}

static void shm_close(void)
{
    if (g_shm) {
        g_shm->magic = 0;            /* 通知消费者连接失效 */
        g_shm->flags &= ~1;
        g_shm->flags &= ~2;
        UnmapViewOfFile(g_shm);
        g_shm = NULL;
    }
    if (g_hMap) { CloseHandle(g_hMap); g_hMap = NULL; }
}

/* 目标地址必须已映射(防止非游戏进程崩溃) */
static int mem_valid(DWORD addr)
{
    MEMORY_BASIC_INFORMATION mbi;
    if (VirtualQuery((LPCVOID)addr, &mbi, sizeof(mbi)) == 0)
        return 0;
    return mbi.State == MEM_COMMIT;
}

/* ---------------- 轮询采集器 ----------------
 * behavior hook 对玩家攻击不触发(仅怪物AI/技能对象),
 * 改为轮询伤害飘字容器: 每次命中/伤害必产生飘字节点 -> 事件
 * dword_415D2B4 -> +8 容器 -> +4 链表最新节点 */
static DWORD WINAPI collector_thread(LPVOID p)
{
    DWORD lastNode = 0, lastC = 0;
    BYTE  lastSnap[16] = { 0 };
    DWORD lastHitTick = 0;
    DWORD firstSeen = 0;
    (void)p;

    while (g_running) {
        /* 每 5 秒打印采集链路状态(诊断) */
        {
            static DWORD s_diag = 0;
            DWORD now3 = GetTickCount();
            if (now3 - s_diag >= 30000) {
                s_diag = now3;
                if (mem_valid(MEM_G_DAMAGE_FONT)) {
                    DWORD m = *(volatile DWORD *)MEM_G_DAMAGE_FONT;
                    DWORD c = m ? *(volatile DWORD *)(m + MEM_CONTAINER_OFF) : 0;
                    DWORD n = c ? *(volatile DWORD *)(c + MEM_HEAD_OFF) : 0;
                    dll_log("DIAG manager=0x%08X container=0x%08X node=0x%08X", m, c, n);
                    if (m) {
                        DWORD d[8];
                        int k;
                        for (k = 0; k < 8; k++) d[k] = *(volatile DWORD *)(m + k * 4);
                        dll_log("DIAG dump %08X %08X %08X %08X %08X %08X %08X %08X",
                                d[0], d[1], d[2], d[3], d[4], d[5], d[6], d[7]);
                    }
                } else {
                    dll_log("DIAG addr invalid");
                }
            }
        }
        if (mem_valid(MEM_G_DAMAGE_FONT)) {
            DWORD manager = *(volatile DWORD *)MEM_G_DAMAGE_FONT;        if (manager) {
            DWORD container = *(volatile DWORD *)(manager + MEM_CONTAINER_OFF);
            if (container) {
                DWORD node = *(volatile DWORD *)(container + MEM_HEAD_OFF);
                if (node && node != container) {
                    BYTE snap[16];
                    DWORD now = GetTickCount();
                    int changed;
                    memcpy(snap, (void *)node, 16);
                    changed = memcmp(snap, lastSnap, 16) != 0;
                    if (node != lastNode || container != lastC || changed) {
                        if (firstSeen && (!lastHitTick || now - lastHitTick > 80)) {
                            vib_collect(VEV_ATTACK, 100, now, 0);
                            vib_collect(VEV_DAMAGE, 3000, now, 0);
                            dll_log("HIT node=0x%08X", node);
                        }
                        firstSeen = 1;
                        lastHitTick = now;
                        lastNode = node;
                        lastC = container;
                        memcpy(lastSnap, snap, 16);
                    }
                } else {
                    if (lastNode && lastHitTick && GetTickCount() - lastHitTick > 500) {
                        vib_collect(VEV_ACTION_END, 0, GetTickCount(), 0);
                        dll_log("ACTION_END (battle over)");
                        lastNode = 0;
                    }
                }
            }
        }
        }
        /* 聚合冲刷: hook 回调只做计数, 此处每 12ms 批量发送(降游戏线程开销) */
        vib_flush_counts();
        vib_flush_rank();
        vib_flush_rank_extra();
        poll_cam_shake();
        /* 背击/破招定位已放弃 (2026-08-15): 静态分析+评分计数器+dstr解密均未定位,
         * 推断由服务器判定 (回包带伤害类型)。诊断函数保留见 hooks.c (poll_rank_events/
         * diag_rank_texts/scan_dstr_text), 不调用。详见 docs/背击破招定位_v19.md */
        Sleep(6);
    }
    return 0;
}

/* ---------------- GDI TextOutW IAT hook(评分文字采集) ----------------
 * 游戏 UI 文字(Combo 计数/评分文字等)走 GDI TextOutW, IAT 槽 0x0571C474
 * 参考 usdof_cn_fix 的 IAT hook 手法 */
#define TEXT_OUT_IAT 0x0571C474

typedef BOOL (WINAPI *FnTextOutW)(HDC, int, int, LPCWSTR, int);
static FnTextOutW g_origTextOutW = NULL;
static volatile LONG g_textHookIn = 0;

/* 与 hooks.c 共享的分类器 */
extern int classify_font_text(const wchar_t *txt, DWORD *strength);
/* 采集输出回调(由 hooks.c 提供) */
extern void vib_flush_counts(void);
extern void vib_flush_rank(void);
extern void vib_flush_rank_extra(void);
extern void vib_collect(int type, DWORD strength, DWORD tick, DWORD count);
static BOOL WINAPI MyTextOutW(HDC hdc, int x, int y, LPCWSTR s, int c)
{
    if (s && c > 0 && c < 96 && g_origTextOutW) {
        wchar_t txt[96];
        DWORD str = 0;
        int ev;
        wcsncpy(txt, s, 95);
        txt[95] = 0;
        ev = classify_font_text(txt, &str);
        if (ev == VEV_RATING || ev == VEV_COMBO || ev == VEV_BACK || ev == VEV_BREAK) {
            vib_collect(ev, str, GetTickCount(), 0);
#ifdef VIB_DEBUG_TXT
            /* 调试版: 评分/连击事件全部记录(不限频) */
            dll_log("TXT %ls -> ev=%d str=%lu (SENT)", txt, ev, str);
#else
            /* 限频日志 */
            if (InterlockedCompareExchange(&g_textHookIn, 1, 0) == 0) {
                static DWORD s_last = 0;
                DWORD now = GetTickCount();
                if (now - s_last >= 1000) {
                    s_last = now;
                    dll_log("TXT %ls -> ev=%d str=%lu", txt, ev, str);
                }
                InterlockedExchange(&g_textHookIn, 0);
            }
#endif
        }
#ifdef VIB_DEBUG_TXT
        else {
            /* 调试版: 未分类文本也输出, 确认 hook 覆盖所有 UI 文字 */
            dll_log("TXT %ls -> ev=%d str=%lu (skip)", txt, ev, str);
        }
#endif
    }
    return g_origTextOutW(hdc, x, y, s, c);
}

static void textout_hook_install(void)
{
    if (mem_valid(TEXT_OUT_IAT)) {
        DWORD *slot = (DWORD *)TEXT_OUT_IAT;
        g_origTextOutW = (FnTextOutW)*slot;
        if (g_origTextOutW && g_origTextOutW != (FnTextOutW)MyTextOutW) {
            *slot = (DWORD)MyTextOutW;
            dll_log("TextOutW IAT hooked (orig=0x%08X)", (DWORD)g_origTextOutW);
        }
    }
}

static void textout_hook_uninstall(void)
{
    if (g_origTextOutW && mem_valid(TEXT_OUT_IAT)) {
        DWORD *slot = (DWORD *)TEXT_OUT_IAT;
        if (*slot == (DWORD)MyTextOutW)
            *slot = (DWORD)g_origTextOutW;
        g_origTextOutW = NULL;
    }
}

/* ---------------- NPK 评分贴图定位(运行时内存扫描) ----------------
 * dstr.dat 解密后 "combo_bonus" 等路径字符串在进程内存中,
 * 扫描字符串地址 -> 扫描代码段引用点 -> 日志输出, 供精准 hook */
static DWORD scan_find_str(const char *needle)
{
    MEMORY_BASIC_INFORMATION mbi;
    BYTE *addr = NULL;
    DWORD len = (DWORD)strlen(needle);
    while (VirtualQuery(addr, &mbi, sizeof(mbi)) && mbi.State == MEM_COMMIT) {
        if (mbi.Protect & (PAGE_READWRITE | PAGE_READONLY | PAGE_WRITECOPY | PAGE_EXECUTE_READWRITE)) {
            BYTE *base = (BYTE *)mbi.BaseAddress;
            SIZE_T sz = mbi.RegionSize;
            SIZE_T i;
            for (i = 0; i + len < sz; i++) {
                if (base[i] == (BYTE)needle[0] && memcmp(base + i, needle, len) == 0) {
                    DWORD found = (DWORD)(ULONG_PTR)(base + i);
                    return found;
                }
            }
        }
        addr = (BYTE *)mbi.BaseAddress + mbi.RegionSize;
        if ((ULONG_PTR)addr < 0x400000) addr = (BYTE *)0x400000;
    }
    return 0;
}

/* 扫描代码段: push imm32 / mov reg,imm32 引用指定地址的指令 */
static void scan_refs(DWORD target, const char *tag)
{
    DWORD ea = 0x00401000;
    DWORD end = 0x05294000;
    DWORD cnt = 0;
    BYTE b;
    DWORD imm;
    if (!mem_valid(0x00401000)) {
        dll_log("SCAN skip (not game process)");
        return;
    }
    dll_log("SCAN refs to 0x%08X (%s):", target, tag);
    while (ea < end - 5) {
        if (!mem_valid(ea)) {
            /* 跳到下一已提交页 */
            MEMORY_BASIC_INFORMATION mbi;
            if (VirtualQuery((LPCVOID)ea, &mbi, sizeof(mbi)) == 0)
                break;
            ea = (DWORD)(ULONG_PTR)mbi.BaseAddress + (DWORD)mbi.RegionSize;
            continue;
        }
        b = *(BYTE *)ea;
        if (b == 0x68) {   /* push imm32 */
            imm = *(DWORD *)(ea + 1);
            if (imm == target) {
                dll_log("  push@0x%08X", ea);
                cnt++;
            }
            ea += 5;
        } else if (b >= 0xB8 && b <= 0xBF) {  /* mov reg, imm32 */
            imm = *(DWORD *)(ea + 1);
            if (imm == target) {
                dll_log("  mov@0x%08X", ea);
                cnt++;
            }
            ea += 5;
        } else {
            ea += 1;
        }
    }
    if (cnt == 0) dll_log("  (no direct refs found)");
}

static void npk_scan(void)
{
    DWORD a1 = scan_find_str("combo_bonus");
    DWORD a2 = scan_find_str("dungeon_rank");
    DWORD a3 = scan_find_str("dungeon_score");
    dll_log("NPK scan: combo_bonus=0x%08X dungeon_rank=0x%08X dungeon_score=0x%08X",
            a1, a2, a3);
    if (a1) scan_refs(a1, "combo_bonus");
    if (a2) scan_refs(a2, "dungeon_rank");
    if (a3) scan_refs(a3, "dungeon_score");
}

/* ---------------- worker ---------------- */

/* 检测 DfoVibration.exe 是否已在运行 */
static int exe_running(void)
{
    HANDLE snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
    PROCESSENTRY32 pe;
    int found = 0;
    if (snap == INVALID_HANDLE_VALUE) return 0;
    pe.dwSize = sizeof(pe);
    if (Process32First(snap, &pe)) {
        do {
            if (!_stricmp(pe.szExeFile, "DfoVibration.exe")) { found = 1; break; }
        } while (Process32Next(snap, &pe));
    }
    CloseHandle(snap);
    return found;
}

/* 傻瓜式: DLL 挂载后自动启动同目录/上级目录的 DfoVibration.exe */
static void autostart_exe(void)
{
    char dllPath[MAX_PATH];
    char exePath[MAX_PATH * 2];
    char *slash;

    if (exe_running()) return;
    if (!GetModuleFileNameA(NULL, dllPath, MAX_PATH)) return;
    slash = strrchr(dllPath, '\\');
    if (!slash) return;
    *slash = 0;
    /* 同目录 */
    _snprintf(exePath, sizeof(exePath), "%s\\DfoVibration.exe", dllPath);
    if (GetFileAttributesA(exePath) == INVALID_FILE_ATTRIBUTES) {
        /* 上级目录(游戏根目录) */
        slash = strrchr(dllPath, '\\');
        if (!slash) return;
        *slash = 0;
        _snprintf(exePath, sizeof(exePath), "%s\\DfoVibration.exe", dllPath);
        if (GetFileAttributesA(exePath) == INVALID_FILE_ATTRIBUTES)
            return;
    }
    ShellExecuteA(NULL, "open", exePath, NULL, NULL, SW_SHOWNORMAL);
}

static DWORD WINAPI worker_thread(LPVOID param)
{
    int lastEnabled = -1;
    VibConfig cfg;
    static int s_autostarted = 0;

    (void)param;
    /* 延迟: 避开 DllMain loader lock 窗口, 此时才可安全调
     * ShellExecuteA/CreateToolhelp32Snapshot 等可能加载 DLL 的 API */
    Sleep(1000);

    /* 用 DLL 自身模块句柄取路径(不能用 NULL, 那是 EXE 路径)。
     * 手动映射注入时 DllMain 可能不触发, 此处兜底: 枚举进程模块
     * 找文件名含 "DfoVibration" 的模块作为自身句柄 */
    if (!g_hDllMod) {
        /* Psapi 动态加载, 避免额外链接依赖 */
        typedef BOOL (WINAPI *FnEnumProcessModules)(HANDLE, HMODULE *, DWORD, DWORD *);
        typedef DWORD (WINAPI *FnGetModuleBaseNameA)(HANDLE, HMODULE, char *, DWORD);
        HMODULE psapi = LoadLibraryA("psapi.dll");
        if (psapi) {
            FnEnumProcessModules fnEnum = (FnEnumProcessModules)
                GetProcAddress(psapi, "EnumProcessModules");
            FnGetModuleBaseNameA fnName = (FnGetModuleBaseNameA)
                GetProcAddress(psapi, "GetModuleBaseNameA");
            if (fnEnum && fnName) {
                HMODULE mods[256];
                DWORD cb = 0;
                if (fnEnum(GetCurrentProcess(), mods, sizeof(mods), &cb)) {
                    DWORD n = cb / sizeof(HMODULE);
                    DWORD i;
                    for (i = 0; i < n && i < 256; i++) {
                        char name[MAX_PATH];
                        if (fnName(GetCurrentProcess(), mods[i], name, MAX_PATH) &&
                            _strnicmp(name, "DfoVibration", 12) == 0) {
                            g_hDllMod = mods[i];
                            break;
                        }
                    }
                }
            }
        }
    }
    GetModuleFileNameA(g_hDllMod, g_iniPath, MAX_PATH);
    {
        char *slash = strrchr(g_iniPath, '\\');
        if (slash) {
            /* 基于 DLL 自身文件名派生 ini/log (如 DfoVibration-V.dll ->
             * DfoVibration-V.ini / DfoVibration-V_dll.log), 多版本共存互不干扰 */
            char *dot = strrchr(slash + 1, '.');
            if (dot) *dot = 0;                      /* 去掉 .dll */
            _snprintf(slash + 1, MAX_PATH - (slash - g_iniPath) - 1,
                      "%s.ini", slash + 1);
            strcpy(g_logPath, g_iniPath);
            dot = strrchr(slash + 1, '.');
            if (dot) *dot = 0;
            _snprintf(slash + 1, MAX_PATH - (slash - g_iniPath) - 1,
                      "%s_dll.log", slash + 1);
            strcpy(g_logPath, g_iniPath);
        }
    }
    log_rotate_check();
    dll_log("========== worker start v3.0 pid=%lu %s ==========", (unsigned long)GetCurrentProcessId(), g_iniPath);

    for (;;) {
        if (!g_running) break;

        /* 延迟扫描评分贴图字符串(等 dstr 解密完成), 仅一次, 不阻塞初始化 */
        {
            static DWORD s_scanAt = 0;
            static int s_scanned = 0;
            if (!s_scanned) {
                if (!s_scanAt) s_scanAt = GetTickCount() + 3000;
                if (GetTickCount() >= s_scanAt) {
                    s_scanned = 1;
                    npk_scan();
                }
            }
        }

        if (g_iniPath[0]) ini_load_config(g_iniPath, &cfg);
        else cfg.enabled = 1;

        /* 状态日志降频: 30 秒一次(不再每 500ms 刷屏) */
        {
            static DWORD s_tick = 0;
            DWORD nw = GetTickCount();
            if (nw - s_tick >= 30000) {
                s_tick = nw;
                dll_log("tick enabled=%d autostart=%d hooks=%d", cfg.enabled, cfg.autostart, hooks_active());
            }
        }

        /* hook 完整性校验(诊断: 降频到 60 秒) */
        {
            static DWORD s_check = 0;
            DWORD now2 = GetTickCount();
            if (hooks_active() && now2 - s_check >= 60000) {
                s_check = now2;
                if (*(BYTE *)g_hookTargets[HOOK_ON_ATTACK] != 0xE9)
                    dll_log("WARN hook overwritten (0x%02X) - runtime self-protection?");
                else
                    dll_log("hook intact");
            }
        }
        /* 自动启动控制面板(仅一次) */
        if (!s_autostarted) {
            s_autostarted = 1;
            if (cfg.autostart)
                autostart_exe();
        }

        if (cfg.enabled != lastEnabled) {
            dll_log("enabled change %d -> %d", lastEnabled, cfg.enabled);
            if (cfg.enabled) {
                /* 先开共享内存(EXE 可见), 再装 hook */
                if (shm_open()) {
                    int n = hooks_install(g_hookTargets, HOOK_COUNT);
                        /* 伤害飘字 hook: 每次伤害/飘字必经 (sub_E010E0) */
                        {
                            static const DWORD hitTarget = 0x00E010E0;
                            int n2 = hooks_install(&hitTarget, 1);
                            /* 震屏 hook: 技能震屏(sub_1E25540) + 真镜头震动(sub_1E268F0) + 读条震屏(sub_1E25760) */
                            static const DWORD shakeTarget = 0x01E25540;
                            int n3 = hooks_install(&shakeTarget, 1);
                            static const DWORD shakeScreenTarget = 0x01E268F0;
                            int n4 = hooks_install(&shakeScreenTarget, 1);
                            static const DWORD readshakeTarget = 0x01E25760;
                            int n5 = hooks_install(&readshakeTarget, 1);
                            /* 怪物死亡 hook: sub_F20BF0 (被击者 HP<=0 确认死亡后调用, 精确) */
                            static const DWORD dieTarget = 0x00F20BF0;
                            int n6 = hooks_install(&dieTarget, 1);
                            /* 直接震屏 hook: sub_1E26F00 (目标容器空时直接写相机字段, 不命中也震) */
                            static const DWORD directShakeTarget = 0x01E26F00;
                            int n7 = hooks_install(&directShakeTarget, 1);
                            int i;
                            for (i = 0; i < HOOK_COUNT; i++)
                                g_shm->apiVersion[i] = g_hookTargets[i];
                            g_shm->flags |= 2;
                            g_ringReady = hooks_active() ? 1 : 0;
                            dll_log("shm ok, hooks installed=%d+%d+%d+%d+%d+%d+%d/%d (评分=内存轮询)", n, n2, n3, n4, n5, n6, n7, HOOK_COUNT + 6);
                            textout_hook_install();
                            if (n + n2 == 0) { /* 全部失败: 记录但保持运行 */
                            }
                        }
                    } else {
                        dll_log("shm_open FAILED");
                    }
                } else {
                    hooks_uninstall();
                    textout_hook_uninstall();
                    g_ringReady = 0;
                    shm_close();
                }
            lastEnabled = cfg.enabled;
        }
        Sleep(500);
    }
    hooks_uninstall();
    g_ringReady = 0;
    shm_close();
    return 0;
}

/* ---------------- 导出 ---------------- */
__declspec(dllexport) void __cdecl DfoVibrationLoaded(void)
{
    if (InterlockedCompareExchange((volatile LONG *)&g_workerStarted, 1, 0) == 0) {
        g_running = 1;
        CreateThread(NULL, 0, worker_thread, NULL, 0, NULL);
    }
}

/* 供注入/控制用: 同义导出 */
__declspec(dllexport) void __cdecl VibPluginLoaded(void)
{
    DfoVibrationLoaded();
}

__declspec(dllexport) void __cdecl VibPluginDeinit(void)
{
    g_running = 0;
}

BOOL WINAPI DllMain(HINSTANCE hDll, DWORD reason, LPVOID reserved)
{
    (void)reserved;
    if (reason == DLL_PROCESS_ATTACH) {
        g_hDllMod = hDll;
        /* 直接建线程(线程体延迟 1s, 避开 loader lock);
         * 官方启动器随后调 DfoVibrationLoaded()(幂等) */
        DisableThreadLibraryCalls(hDll);
        if (InterlockedCompareExchange((volatile LONG *)&g_workerStarted, 1, 0) == 0) {
            g_running = 1;
            g_hWorker = CreateThread(NULL, 0, worker_thread, NULL, 0, NULL);
            g_hCollector = CreateThread(NULL, 0, collector_thread, NULL, 0, NULL);
        }
    } else if (reason == DLL_PROCESS_DETACH) {
        g_running = 0;
        if (g_hWorker) { WaitForSingleObject(g_hWorker, 1500); g_hWorker = NULL; }
        if (g_hCollector) { WaitForSingleObject(g_hCollector, 1500); g_hCollector = NULL; }
        hooks_uninstall();
        textout_hook_uninstall();
        g_ringReady = 0;
        shm_close();
    }
    return TRUE;
}
