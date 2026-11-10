@echo off
rem Jumps straight into one part of SpiritScape, for testing.
rem
rem Scenarios keep their saves in %APPDATA%\SpiritScape\scenario-tests, apart
rem from your own game, so trying one can never overwrite it.
setlocal
cd /d "%~dp0"
set "SAVES=%APPDATA%\SpiritScape\scenario-tests"

echo.
echo  SpiritScape scenarios
echo  ---------------------
echo   1  The quarry             Stone, and a Quarrying Spirit
echo   2  The Mason's Yard       Dressed Stone and Keystones
echo   3  A remote outpost       Working far from home
echo   4  Production chains      Planks, panels and beams
echo   5  Trade Caravans         Sending goods away, and what comes back
echo   6  The Kiln               Clay, charcoal and Bricks
echo   7  A quiet hour           Spirits, their names and small moments
echo   8  Winter                 Snow, and the frozen south river
echo   9  The Hearth             The first ending
echo  10  The Still Boat         The other ending
echo  11  The whole map          Everything built
echo.
set /p CHOICE="Which one? "

set "ARGS="
if "%CHOICE%"=="1" set "ARGS=--scenario quarry_start"
if "%CHOICE%"=="2" set "ARGS=--scenario mason_yard"
if "%CHOICE%"=="3" set "ARGS=--scenario outpost_establishment"
if "%CHOICE%"=="4" set "ARGS=--scenario panel_workshop"
if "%CHOICE%"=="5" set "ARGS=--scenario caravan_yard"
if "%CHOICE%"=="6" set "ARGS=--scenario kiln_yard"
if "%CHOICE%"=="7" set "ARGS=--scenario quiet_hour"
if "%CHOICE%"=="8" set "ARGS=--new-game --skip-minutes 95"
if "%CHOICE%"=="9" set "ARGS=--scenario last_stage"
if "%CHOICE%"=="10" set "ARGS=--scenario last_boat"
if "%CHOICE%"=="11" set "ARGS=--scenario full_map"
if "%ARGS%"=="" (
  echo That isn't one of the numbers above.
  pause
  exit /b 1
)

call "%~dp0play.bat" %ARGS% --save-dir "%SAVES%"
