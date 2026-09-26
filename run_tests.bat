@echo off
REM Test runner for Sorahk on Windows
REM ★架构重构 A (2026-09-27): 一轮只跑一遍全量测试。旧版连跑 4 遍
REM (debug lib / 集成 / 全量 --nocapture / 全量 --release), 测试时间无谓 x4。
REM 默认走 iter 快速档 (与日常构建同档, 复用缓存); 交付复核: run_tests.bat --release
REM 参数原样透传给 cargo test (例: run_tests.bat --lib / run_tests.bat --release)。

echo ============================================
echo   Sorahk Test Suite (single pass)
echo ============================================
echo.

set MODE=--profile iter
if "%1"=="--release" set MODE=--release
if "%1"=="--debug" set MODE=

echo [1/1] cargo test %MODE% %2 %3 %4 ...
cargo test %MODE% %2 %3 %4
if %ERRORLEVEL% NEQ 0 (
    echo Tests FAILED!
    exit /b 1
)
echo.
echo ============================================
echo   All tests passed successfully!
echo ============================================
