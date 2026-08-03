@echo off
setlocal EnableDelayedExpansion
chcp 65001 >nul
echo ==================================================
echo  ZPos Kasir - Build Lokal Windows (66f2360)
echo ==================================================
echo.

REM ---------- 1. Pilih repo ----------
set "REPO=%CD%"
if not exist "%REPO%\src-tauri\Cargo.toml" (
  echo [!] Cargo.toml tidak ditemukan di: %REPO%
  echo     Jalankan script ini DARI folder repo zpos-kasir.
  cd /d "%~dp0"
  set "REPO=%CD%"
)
echo Repo : %REPO%
cd /d "%REPO%"
echo.

REM ---------- 2. Cek git + pull ----------
echo [1/5] Git pull (ambil source terbaru 66f2360)...
git fetch origin main >nul 2>&1
git reset --hard origin/main >nul 2>&1
for /f "delims=" %%L in ('git log -1 --oneline') do set "HEADLINE=%%L"
echo       HEAD: %HEADLINE%
echo.

REM ---------- 3. Cek Rust ----------
echo [2/5] Cek toolchain Rust/tauri-cli...
where cargo >nul 2>&1 || ( echo [X] cargo tidak ada. Install Rust: https://rustup.rs ; lalu restart CMD. & pause & exit /b 1 )
where cargo-tauri >nul 2>&1 || (
  echo      cargo-tauri belum ada, install dulu (1-3 mnt)...
  cargo install tauri-cli --locked || ( echo [X] Gagal install tauri-cli. & pause & exit /b 1 )
)
echo       OK.
echo.

REM ---------- 4. Build ----------
echo [3/5] Build exe (release, --no-bundle; pertama 5-15 mnt karena lto=true)...
cd /d "%REPO%\src-tauri"
cargo tauri build --no-bundle
if errorlevel 1 ( echo [X] Build GAGAL. & pause & exit /b 1 )
echo       Build OK.
echo.

REM ---------- 5. Lokasi hasil ----------
set "EXE=%REPO%\src-tauri\target\release\zpos-kasir.exe"
if not exist "%EXE%" (
  echo [X] exe tidak ditemukan di %EXE%
  pause & exit /b 1
)
echo [4/5] Hasil: %EXE%
echo.

REM ---------- 6. Reset WebView2 cache (WAJIB biar index.html baru kepakai) ----------
echo [5/5] Reset cache WebView2 (hapus %APPDATA%\my.id.zpos.kasir.app)...
set "CACHE=%APPDATA%\my.id.zpos.kasir.app"
if exist "%CACHE%" (
  rmdir /s /q "%CACHE%"
  echo       Cache dihapus: %CACHE%
) else (
  echo       Cache tak ada, lanjut.
)
echo.

echo ==================================================
echo  SELESAI. Jalankan exe baru:
echo    "%EXE%"
echo  Lalu tes QR Login: buka setelan - Tampilkan QR Login,
echo  pastikan QR memenuhi modal (tidak terpotong), scan pakai HP Z One.
echo ==================================================
pause
