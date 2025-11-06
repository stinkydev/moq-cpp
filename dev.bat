@echo off
REM Development helper script for moq-cpp (Windows)

if "%1"=="fmt" goto fmt
if "%1"=="check" goto check
if "%1"=="fix" goto fix
if "%1"=="build" goto build
if "%1"=="clean" goto clean
if "%1"=="setup-hooks" goto setup-hooks
goto usage

:fmt
echo 🎨 Formatting Rust code...
cargo fmt
echo ✅ Code formatted!
goto end

:check
echo 🔍 Running format check...
cargo fmt --check
if errorlevel 1 goto error
echo 🔍 Running clippy...
cargo clippy --all-targets --all-features -- -D warnings
if errorlevel 1 goto error
echo ✅ All checks passed!
goto end

:fix
echo 🔧 Auto-fixing code...
cargo fmt
cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
echo ✅ Auto-fixes applied!
goto end

:build
echo 🔨 Building project...
cargo fmt
if errorlevel 1 goto error
cargo clippy --all-targets --all-features -- -D warnings
if errorlevel 1 goto error
cmake --build build --config Release
if errorlevel 1 goto error
echo ✅ Build complete!
goto end

:clean
echo 🧹 Cleaning build artifacts...
cargo clean
if exist build rmdir /s /q build
echo ✅ Clean complete!
goto end

:setup-hooks
echo ⚙️ Setting up Git hooks...
git config core.hooksPath .githooks
echo ✅ Git hooks configured!
goto end

:usage
echo Usage: %0 {fmt^|check^|fix^|build^|clean^|setup-hooks}
echo.
echo Commands:
echo   fmt         - Format Rust code
echo   check       - Check formatting and run clippy
echo   fix         - Auto-fix formatting and clippy issues
echo   build       - Format, check, and build project
echo   clean       - Clean build artifacts
echo   setup-hooks - Configure Git pre-commit hooks
exit /b 1

:error
echo ❌ Command failed!
exit /b 1

:end