@echo off
:: DemonNapse - disinstallazione (eseguire come AMMINISTRATORE). Rimette tutto come prima.
setlocal

net session >nul 2>&1
if errorlevel 1 (
    echo Serve l'esecuzione come amministratore.
    pause
    exit /b 1
)

echo [1/2] Fermo il demone e tolgo l'attivita' pianificata...
for %%T in ("DemonNapse" "RazerLite" "RazerLiteWallpaper") do (
    schtasks /End /TN %%T >nul 2>&1
    schtasks /Delete /F /TN %%T >nul 2>&1
)
taskkill /F /IM demonnapse.exe >nul 2>&1
powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'demonnapse.py|razerlite.py|wallpaper.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }" >nul 2>&1

echo [2/2] Servizi Razer e avvio di Synapse come erano prima dell'installazione...
:: razer_backup.txt lo scrive install.cmd; se manca (installato con una versione vecchia) servizi automatici e avvio standard di Synapse
powershell -NoProfile -Command "$f='%~dp0razer_backup.txt'; $saved=Test-Path $f; $b=@{}; $appr=''; $q=[char]34; $run=$q+'C:\Program Files\Razer\RazerAppEngine\RazerAppEngine.exe'+$q+' --url-params=apps=synapse,chroma-app --launch-force-hidden=synapse,chroma-app --autoStart=1'; if ($saved) { $run=''; foreach ($l in Get-Content $f) { if ($l.StartsWith('run|')) { $run=$l.Substring(4) } elseif ($l.StartsWith('approved|')) { $appr=$l.Substring(9) } elseif ($l.StartsWith('service|')) { $p=$l.Split('|'); $b[$p[1]]=$p[2] } } }; foreach ($s in 'RzActionSvc','RazerExperienceService','Razer Game Manager Service 3','RGS','Razer Chroma SDK Server','Razer Chroma SDK Service','Razer Chroma SDK Diagnostic Service','Razer Chroma Stream Server','Razer Elevation Service') { $t=$b[$s]; if (-not $saved) { $t='auto' }; if (-not $t) { continue }; sc.exe config $s start= $t | Out-Null; if ($t -like '*auto') { sc.exe start $s | Out-Null } }; if ($run) { Set-ItemProperty 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run' RazerAppEngine $run }; $k='HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'; if ($appr) { Set-ItemProperty $k RazerAppEngine ([byte[]]($appr.Split('-') | ForEach-Object { [Convert]::ToByte($_, 16) })) -Type Binary } else { Remove-ItemProperty $k RazerAppEngine -EA 0 }; if ($saved) { Remove-Item $f }"

echo.
echo Fatto. Al prossimo accesso Windows tornera' com'era prima di DemonNapse.
pause
