@echo off
:: DemonNapse - uninstall (run as ADMINISTRATOR). Puts everything back as it was.
setlocal

net session >nul 2>&1
if errorlevel 1 (
    echo Run this as administrator.
    pause
    exit /b 1
)

echo [1/2] Stopping the daemon and removing the scheduled task...
schtasks /End /TN "DemonNapse" >nul 2>&1
schtasks /Delete /F /TN "DemonNapse" >nul 2>&1
taskkill /F /IM demonnapse.exe >nul 2>&1

echo [2/2] Razer services and Synapse's autostart as they were before the install...
:: razer_backup.txt is written by install.cmd; without it (installed with an older version) services go automatic and Synapse gets its standard autostart
powershell -NoProfile -Command "$f='%~dp0razer_backup.txt'; $saved=Test-Path $f; $b=@{}; $appr=''; $q=[char]34; $run=$q+'C:\Program Files\Razer\RazerAppEngine\RazerAppEngine.exe'+$q+' --url-params=apps=synapse,chroma-app --launch-force-hidden=synapse,chroma-app --autoStart=1'; if ($saved) { $run=''; foreach ($l in Get-Content $f) { if ($l.StartsWith('run|')) { $run=$l.Substring(4) } elseif ($l.StartsWith('approved|')) { $appr=$l.Substring(9) } elseif ($l.StartsWith('service|')) { $p=$l.Split('|'); $b[$p[1]]=$p[2] } } }; foreach ($s in 'RzActionSvc','RazerExperienceService','Razer Game Manager Service 3','RGS','Razer Chroma SDK Server','Razer Chroma SDK Service','Razer Chroma SDK Diagnostic Service','Razer Chroma Stream Server','Razer Elevation Service') { $t=$b[$s]; if (-not $saved) { $t='auto' }; if (-not $t) { continue }; sc.exe config $s start= $t | Out-Null; if ($t -like '*auto') { sc.exe start $s | Out-Null } }; if ($run) { Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' RazerAppEngine $run }; $k='HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; if ($appr) { Set-ItemProperty $k RazerAppEngine ([byte[]]($appr.Split('-') | ForEach-Object { [Convert]::ToByte($_, 16) })) -Type Binary } else { Remove-ItemProperty $k RazerAppEngine -EA 0 }; if ($saved) { Remove-Item $f }"

echo.
echo Done. From the next logon Windows is back to how it was before DemonNapse.
pause
