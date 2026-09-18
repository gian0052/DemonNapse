@echo off
:: DemonNapse - disinstallazione (eseguire come AMMINISTRATORE). Rimette tutto come prima.
setlocal

net session >nul 2>&1
if errorlevel 1 (
    echo Serve l'esecuzione come amministratore.
    pause
    exit /b 1
)

echo [1/3] Fermo il demone e tolgo l'attivita' pianificata...
for %%T in ("DemonNapse" "RazerLite" "RazerLiteWallpaper") do (
    schtasks /End /TN %%T >nul 2>&1
    schtasks /Delete /F /TN %%T >nul 2>&1
)
powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'demonnapse.py|razerlite.py|wallpaper.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }" >nul 2>&1

echo [2/3] Servizi Razer di nuovo automatici...
for %%S in ("RzActionSvc" "RazerExperienceService" "Razer Game Manager Service 3" "RGS" "Razer Chroma SDK Server" "Razer Chroma SDK Service" "Razer Chroma SDK Diagnostic Service") do (
    sc config %%S start= auto >nul 2>&1
    sc start %%S >nul 2>&1
)

echo [3/3] Synapse di nuovo in avvio automatico...
reg add "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v RazerAppEngine /t REG_SZ /f ^
    /d "\"C:\Program Files\Razer\RazerAppEngine\RazerAppEngine.exe\" --url-params=apps=synapse,chroma-app --launch-force-hidden=synapse,chroma-app --autoStart=1" >nul

echo.
echo Fatto. Al prossimo accesso Windows tornera' com'era prima di DemonNapse.
pause
