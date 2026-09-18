@echo off
:: DemonNapse - installazione (eseguire come AMMINISTRATORE: tasto destro > Esegui come amministratore)
setlocal
set DIR=%~dp0
:: l'eseguibile della release sta accanto a questo file; quello compilato da sorgente in target\release
set "EXE=%DIR%demonnapse.exe"
if not exist "%EXE%" set "EXE=%DIR%target\release\demonnapse.exe"
if not exist "%EXE%" ( echo demonnapse.exe non trovato: scaricalo dalla release oppure compila con "cargo build --release" & pause & exit /b 1 )

net session >nul 2>&1
if errorlevel 1 (
    echo Serve l'esecuzione come amministratore.
    pause
    exit /b 1
)

echo [1/4] Attivita' pianificata "DemonNapse" all'accesso, con privilegi elevati...
schtasks /End /TN "DemonNapse" >nul 2>&1
schtasks /End /TN "RazerLite" >nul 2>&1
schtasks /End /TN "RazerLiteWallpaper" >nul 2>&1
taskkill /F /IM demonnapse.exe >nul 2>&1
:: anche il vecchio demone in Python, se e' ancora installato
powershell -NoProfile -Command "Get-CimInstance Win32_Process | Where-Object { $_.CommandLine -match 'demonnapse.py|experimental\wallpaper.py' } | ForEach-Object { Stop-Process -Id $_.ProcessId -Force }" >nul 2>&1
schtasks /Create /F /TN "DemonNapse" /SC ONLOGON /DELAY 0000:10 /RL HIGHEST /IT ^
    /TR "\"%EXE%\"" >nul
if errorlevel 1 ( echo   ERRORE nella creazione dell'attivita' & pause & exit /b 1 )
:: vecchi nomi / esperimenti: tolgo le attivita' se esistono
schtasks /Delete /F /TN "RazerLite" >nul 2>&1
schtasks /Delete /F /TN "RazerLiteWallpaper" >nul 2>&1

echo [2/4] Tolgo Synapse e Chroma dall'avvio automatico di Windows...
reg delete "HKCU\Software\Microsoft\Windows\CurrentVersion\Run" /v RazerAppEngine /f >nul 2>&1

echo [3/4] Servizi Razer su avvio manuale (li avvia il demone quando apri Synapse)...
for %%S in ("RzActionSvc" "RazerExperienceService" "Razer Game Manager Service 3" "RGS" "Razer Chroma SDK Server" "Razer Chroma SDK Service" "Razer Chroma SDK Diagnostic Service" "Razer Chroma Stream Server" "Razer Elevation Service") do (
    sc config %%S start= demand >nul 2>&1
)

echo [4/4] Avvio il demone adesso...
schtasks /Run /TN "DemonNapse" >nul


echo.
echo Fatto. Il demone parte a ogni accesso; log in %DIR%demonnapse.log
echo Per tornare indietro: uninstall.cmd
pause
