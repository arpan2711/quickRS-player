@echo off
rem Converts every Rocksmith .psarc song in a folder (and its subfolders) to feedpaks, with quickrs-cli.exe.
rem Double-click it; it asks for the two folders. Songs already converted are skipped, so run it again after
rem buying or adding songs. (No parenthesised blocks: a path like "Program Files (x86)" would break them.)
setlocal
cd /d "%~dp0"
title quickRS: convert songs to feedpaks

if exist "%~dp0keys.json" goto keys_ok
echo keys.json is missing. Copy keys.example.json to keys.json, next to this file, and fill in the keys
echo first: see README.txt, "Keys".
echo.
pause
exit /b 1
:keys_ok

set "STEAMDLC=C:\Program Files (x86)\Steam\steamapps\common\Rocksmith2014\dlc"
echo Which folder has your .psarc songs?
if exist "%STEAMDLC%\" goto has_steam
echo For example: %STEAMDLC%
set "STEAMDLC="
goto ask_from
:has_steam
echo Press Enter for Rocksmith's own: %STEAMDLC%
:ask_from
set "FROM="
set /p "FROM=> "
if not defined FROM set "FROM=%STEAMDLC%"
if not defined FROM goto ask_from
set "FROM=%FROM:"=%"
if exist "%FROM%\" goto from_ok
echo "%FROM%" isn't a folder.
goto ask_from
:from_ok

for %%I in ("%FROM%\..") do set "PARENT=%%~fI"
for %%I in ("%FROM%") do set "LEAF=%%~nxI"
set "DEFTO=%FROM%\feedpak"
if /i "%LEAF%"=="dlc" set "DEFTO=%PARENT%\feedpak"
echo.
echo Where should the feedpaks go?
echo Press Enter for: %DEFTO%
if /i "%LEAF%"=="dlc" echo A "feedpak" folder next to "dlc" is found by quickRS by itself.
set "TO="
set /p "TO=> "
if not defined TO set "TO=%DEFTO%"
set "TO=%TO:"=%"

echo.
"%~dp0quickrs-cli.exe" --keys "%~dp0keys.json" convert-folder "%FROM%" "%TO%"
echo.
echo Feedpaks are in: %TO%
pause
