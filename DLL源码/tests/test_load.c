/* 冒烟测试: 加载 DfoVibration.dll, 验证导出 + 共享内存创建 + 卸载安全 */
#include <windows.h>
#include <stdio.h>

static LONG WINAPI crash_handler(EXCEPTION_POINTERS *ep)
{
    fprintf(stderr, "EXCEPTION at 0x%p code 0x%08X\n",
            ep->ExceptionRecord->ExceptionAddress,
            (unsigned)ep->ExceptionRecord->ExceptionCode);
    fflush(stderr);
    return EXCEPTION_EXECUTE_HANDLER;
}

int main(int argc, char **argv)
{
    HMODULE h;
    FARPROC fn;
    HANDLE hMap;
    int ok = 0;

    SetUnhandledExceptionFilter(crash_handler);
    setvbuf(stdout, NULL, _IONBF, 0);
    setvbuf(stderr, NULL, _IONBF, 0);

    if (argc < 2) { fprintf(stderr, "usage: test_load <dll>\n"); return 2; }

    h = LoadLibraryA(argv[1]);
    if (!h) { fprintf(stderr, "LoadLibrary FAILED err=%lu\n", GetLastError()); return 1; }
    fprintf(stderr, "LoadLibrary OK base=0x%p\n", h);

    fn = GetProcAddress(h, "DfoVibrationLoaded");
    if (!fn) { fprintf(stderr, "export DfoVibrationLoaded NOT FOUND\n"); FreeLibrary(h); return 1; }
    fprintf(stderr, "export DfoVibrationLoaded OK\n");

    fn();
    fprintf(stderr, "DfoVibrationLoaded() called, waiting 3s for worker...\n");
    Sleep(3000);

    hMap = OpenFileMappingA(FILE_MAP_ALL_ACCESS, FALSE, "Local\\DfoVibrationShm");
    if (hMap) {
        fprintf(stderr, "shared memory EXISTS (DLL worker running)\n");
        ok = 1;
        CloseHandle(hMap);
    } else {
        fprintf(stderr, "shared memory NOT found (worker maybe not started or ini disabled)\n");
    }

    if (!FreeLibrary(h)) fprintf(stderr, "FreeLibrary FAILED\n");
    else fprintf(stderr, "FreeLibrary OK (uninstall path safe)\n");
    return ok ? 0 : 3;
}
