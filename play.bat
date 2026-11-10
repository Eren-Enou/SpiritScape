@echo off
rem Starts SpiritScape. Double-click this file, or run it from a terminal.
rem
rem Your game is saved in %APPDATA%\SpiritScape. This script never touches it.
rem Anything you add goes to the game, for example:  play.bat --new-game
setlocal
cd /d "%~dp0"

if exist "spirit_game.exe" (
  set "GAME=spirit_game.exe"
  goto run
)

where cargo >nul 2>nul
if errorlevel 1 (
  echo SpiritScape is built with Rust, and Rust isn't installed.
  echo Install it from https://rustup.rs and then run this again.
  pause
  exit /b 1
)
echo Building SpiritScape. The first time takes several minutes; after that it's quick.
cargo build --release -p spirit_game
if errorlevel 1 (
  echo.
  echo The build failed. The messages above say why.
  pause
  exit /b 1
)
set "GAME=target\release\spirit_game.exe"

:run
"%GAME%" %*
if errorlevel 1 (
  echo.
  echo SpiritScape stopped with an error. The messages above say why.
  echo Your saved game is still in %APPDATA%\SpiritScape.
  pause
  exit /b 1
)
