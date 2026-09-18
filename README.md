# DemonNapse

![License: MIT](https://img.shields.io/badge/license-MIT-green)
![Python 3.10+](https://img.shields.io/badge/python-3.10%2B-blue)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6)
![Tested on: Basilisk V3 35K](https://img.shields.io/badge/tested%20on-Basilisk%20V3%2035K-44D62C)

Lightweight replacement for Razer Synapse's RGB engine on Windows. Keep designing your
profiles and Chroma Studio effects in Synapse, close it, and a ~6 MB Python daemon takes
over: one RGB effect per on-board profile, switched with the profile button under the mouse,
with Synapse and all Razer services shut down.

> Independent project, **not affiliated with or endorsed by Razer Inc.** Razer, Synapse and
> Chroma are trademarks of Razer Inc.

## Why

Synapse 4 cannot bind a Chroma Studio effect to an on-board profile (confirmed by Razer
support: the mouse memory stores keys and DPI, never lighting), and the app plus its 14
background services sit at 600–800 MB of RAM. DemonNapse fixes both.

| | Synapse 4 + services | DemonNapse |
|---|---|---|
| RAM (as shown by Task Manager) | ~650 MB + 168 MB | ~6–27 MB |
| CPU (audio-reactive effect) | — | ~0.4 % of one core |
| Chroma Studio effect per on-board profile | no | yes |

## How it works

1. **Synapse stays your editor.** Profiles, key bindings, DPI stages and Chroma Studio effects
   are designed in Synapse/Chroma as usual. Synapse writes all of it to its own log files.
2. **The daemon reads those logs** (`extract_effects.py`): the layers of every Chroma Studio
   effect (static, audio meter, reactive) and the on-board slot of every profile.
3. **Profiles are matched to effects by name**: profile `CS2` → effect `Chroma Effect CS2`,
   `DOTA 2` → `Chroma Effect DOTA`. Words in common win; `effect_overrides` in `config.json`
   forces a match when the names differ.
4. **The mouse tells the daemon which profile is active** (vendor HID register `0x05/0x84`,
   plus the DPI/scroll events the firmware sends on every switch). The daemon renders the
   matching effect frame by frame over HID (`0x0F/0x03`, per-LED) — no Razer software needed.
   The "Hybrid Storage" slot (white indicator) is skipped automatically by jumping to the
   next configured slot (`0x05/0x04`), so a 3-profile setup is 3 clean clicks.
5. **Lifecycle.** At start the daemon kills leftover Razer processes and stops the Razer
   services. When you launch Synapse it notices within ~1.5 s, starts the services, releases
   the mouse and waits. When you close Synapse it re-reads effects and profiles, stops the
   services and takes the mouse back. You never touch a terminal.

Supported layers: **Static**, **Audio Meter** (system output peak via
`IAudioMeterInformation`, no audio capture), **Reactive** (mouse button flash). When an
effect has an Audio Meter, it has priority: static/reactive layers only show after
`audio_idle_delay` seconds of silence.

## Requirements

- Windows 10/11, Python 3.10+ on `PATH` (`pythonw.exe` must be found by `where pythonw`)
- Razer Synapse 4 installed (used only as the editor; it is not running while the daemon works)
- `pip install -r requirements.txt`
- A supported mouse (see below)

## Install

1. In Synapse: put your profiles in on-board memory slots, name a Chroma Studio effect with a
   word in common with each profile, **modify a layer after renaming** (Synapse only logs the
   new name when something changes), then quit Synapse completely (tray → Exit).
2. `python extract_effects.py` — check that every effect and profile is listed.
3. Right-click `install.cmd` → **Run as administrator**. It creates a scheduled task at logon
   with elevated rights (needed to stop/start Razer services), removes Synapse from Windows
   autostart, sets the Razer services to manual start and launches the daemon.

Log: `demonnapse.log` next to the scripts (native crashes, if any, leave a traceback in
`demonnapse.crash.log`). Debug run: `python demonnapse.py -v`.

To go back exactly to how things were: `uninstall.cmd` as administrator.

## Configuration (`config.json`)

| key | meaning |
|---|---|
| `device.pid` / `device.leds` | USB product id and LED count of your mouse |
| `device.chroma_grid` | Chroma Studio grid position `"row,col"` → physical LED index |
| `effect_overrides` | `{"Profile name": "Chroma Effect X"}` when names have nothing in common |
| `skip_slots` | on-board slots to skip (default `[1]`, the Hybrid Storage slot) |
| `audio_gain`, `audio_silence_floor`, `audio_idle_delay` | Audio Meter tuning |

## Supported devices

Verified on the **Razer Basilisk V3 35K (wired, PID `0x00CB`)** only: control interface,
per-LED protocol, LED map and profile registers were all reverse-engineered and tested on that
mouse. Other Razer mice speaking the same protocol (most of them, see the OpenRazer wiki) will
probably work with a different `pid`, `leds` and `chroma_grid`; `tools/sniff.py` shows what
your mouse sends when you press the profile button. Reports and pull requests welcome.

## Things you should know before installing

- The daemon **runs elevated**, **kills `RazerAppEngine.exe`** and **stops the Razer Windows
  services** while it owns the mouse. Read `demonnapse.py` (it is short) if that worries you.
- It depends on the **format of Synapse 4's log files**, which Razer can change at any time.
  If an update breaks extraction, the daemon keeps the last known effects.
- It uses only user-mode APIs (`hidapi` feature reports, `GetAsyncKeyState`, WASAPI meter),
  the same approach as OpenRGB/OpenRazer. Not tested against anti-cheat systems.
- Chroma Studio layers other than Static/Audio Meter/Reactive are ignored with a warning.

## Acknowledgements

The HID packet format comes from the
[OpenRazer](https://github.com/openrazer/openrazer) reverse-engineering wiki. The profile
registers (`0x05/0x80..0x84`, `0x05/0x04`) were found by probing.

## License

MIT.
