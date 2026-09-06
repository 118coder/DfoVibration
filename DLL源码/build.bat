@echo off
rem ============================================================
rem DFO Vibration Plugin - build script v1.3 (LLVM-MinGW, x86)
rem output -> release\
rem ============================================================
setlocal

set TOOL=C:\Users\12290\AppData\Local\Microsoft\WinGet\Packages\MartinStorsjo.LLVM-MinGW.UCRT_Microsoft.Winget.Source_8wekyb3d8bbwe\llvm-mingw-20260602-ucrt-x86_64\bin
set ROOT=%~dp0
set OUT=%ROOT%release
set IMG=%ROOT%third_party\imgui-master
set DX9=%ROOT%third_party\dx9\include

if not exist "%OUT%" mkdir "%OUT%"

echo [1/2] Building DfoVibration.dll (x86, C) ...
"%TOOL%\gcc.exe" -m32 -O2 -Wall -Wextra -o "%OUT%\DfoVibration.dll" "%ROOT%dll\dfovib.c" "%ROOT%dll\hooks.c" "%ROOT%common\ini_util.c" "%ROOT%dll\hooks_asm.s" -shared -static-libgcc || goto :fail

set LIBS=-I"%IMG%" -I"%IMG%\backends" -I"%DX9%" -lgdi32 -luser32 -lcomctl32 -lcomdlg32 -lshell32 -ladvapi32 -ld3d9 -ldwmapi -static-libgcc -static-libstdc++

echo [2/3] Building DfoVibration.exe (x86, C++ ImGui+mapper) ...
"%TOOL%\g++.exe" -m32 -O2 -Wall -Wextra -o "%OUT%\DfoVibration.exe" "%ROOT%exe\main.cpp" "%ROOT%exe\engine.cpp" "%ROOT%exe\xinput.cpp" "%ROOT%exe\memmode.cpp" "%ROOT%exe\padmap.cpp" "%ROOT%exe\mapper.cpp" "%ROOT%exe\rawinput.cpp" "%ROOT%exe\inject.cpp" "%ROOT%common\ini_util.cpp" "%ROOT%common\toml.cpp" "%IMG%\imgui.cpp" "%IMG%\imgui_draw.cpp" "%IMG%\imgui_tables.cpp" "%IMG%\imgui_widgets.cpp" "%IMG%\backends\imgui_impl_dx9.cpp" "%IMG%\backends\imgui_impl_win32.cpp" %LIBS% || goto :fail

echo [3/3] Building DfoVibration_debug.exe (x86, auto console) ...
"%TOOL%\g++.exe" -m32 -O2 -Wall -Wextra -DVIB_DEBUG_ALWAYS=1 -o "%OUT%\DfoVibration_debug.exe" "%ROOT%exe\main.cpp" "%ROOT%exe\engine.cpp" "%ROOT%exe\xinput.cpp" "%ROOT%exe\memmode.cpp" "%ROOT%exe\padmap.cpp" "%ROOT%exe\mapper.cpp" "%ROOT%exe\rawinput.cpp" "%ROOT%exe\inject.cpp" "%ROOT%common\ini_util.cpp" "%ROOT%common\toml.cpp" "%IMG%\imgui.cpp" "%IMG%\imgui_draw.cpp" "%IMG%\imgui_tables.cpp" "%IMG%\imgui_widgets.cpp" "%IMG%\backends\imgui_impl_dx9.cpp" "%IMG%\backends\imgui_impl_win32.cpp" %LIBS% || goto :fail

echo.
echo ============ BUILD OK ============
echo Output: %OUT%
exit /b 0

:fail
echo.
echo BUILD FAILED.
exit /b 1
