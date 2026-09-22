@echo off
:: DemonNapse - install (run as ADMINISTRATOR: right click > Run as administrator)
setlocal
set DIR=%~dp0
:: the release executable sits next to this file; one built from source is in target\release
set "EXE=%DIR%demonnapse.exe"
if not exist "%EXE%" set "EXE=%DIR%target\release\demonnapse.exe"
if not exist "%EXE%" ( echo demonnapse.exe not found: download it from the release or build it with "cargo build --release" & pause & exit /b 1 )

net session >nul 2>&1
if errorlevel 1 (
    echo Run this as administrator.
    pause
    exit /b 1
)

echo [0/4] Saving how the Razer services and Synapse's autostart are set (for uninstall.cmd)...
:: first install only: on a reinstall the services are already manual and the backup would be wrong
powershell -NoProfile -Command "$f='%DIR%razer_backup.txt'; if ((Test-Path $f) -or (Get-ScheduledTask -TaskName 'DemonNapse' -EA 0)) { exit }; $o=@(); foreach ($s in 'RzActionSvc','RazerExperienceService','Razer Game Manager Service 3','RGS','Razer Chroma SDK Server','Razer Chroma SDK Service','Razer Chroma SDK Diagnostic Service','Razer Chroma Stream Server','Razer Elevation Service') { $k=Get-ItemProperty ('HKLM:\SYSTEM\CurrentControlSet\Services\'+$s) -EA 0; if (-not $k) { continue }; $t=@{2='auto';3='demand';4='disabled'}[[int]$k.Start]; if ($t -eq 'auto' -and $k.DelayedAutostart -eq 1) { $t='delayed-auto' }; if ($t) { $o+=('service|'+$s+'|'+$t) } }; $o+=('run|'+(Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' -EA 0).RazerAppEngine); $a=(Get-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run' -EA 0).RazerAppEngine; $o+=('approved|'+$(if ($a) { [BitConverter]::ToString($a) })); Set-Content -Encoding utf8 $f $o"

echo [1/4] Scheduled task "DemonNapse" at logon, with elevated rights...
schtasks /End /TN "DemonNapse" >nul 2>&1
taskkill /F /IM demonnapse.exe >nul 2>&1
schtasks /Create /F /TN "DemonNapse" /SC ONLOGON /DELAY 0000:10 /RL HIGHEST /IT ^
    /TR "\"%EXE%\"" >nul
if errorlevel 1 ( echo   ERROR creating the scheduled task & pause & exit /b 1 )

echo [2/4] Removing Synapse and Chroma from Windows autostart...
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v RazerAppEngine /f >nul 2>&1

echo [3/4] Razer services set to manual start (the daemon starts them when you open Synapse)...
for %%S in ("RzActionSvc" "RazerExperienceService" "Razer Game Manager Service 3" "RGS" "Razer Chroma SDK Server" "Razer Chroma SDK Service" "Razer Chroma SDK Diagnostic Service" "Razer Chroma Stream Server" "Razer Elevation Service") do (
    sc config %%S start= demand >nul 2>&1
)

echo [4/4] Starting the daemon now...
schtasks /Run /TN "DemonNapse" >nul


echo.
echo Done. The daemon starts at every logon; log in %DIR%demonnapse.log
echo To go back: uninstall.cmd
pause
