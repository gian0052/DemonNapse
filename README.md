# DemonNapse

![License: MIT](https://img.shields.io/badge/license-MIT-green)
![Rust](https://img.shields.io/badge/rust-single%20450%20KB%20exe-orange)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6)
![Tested on: Basilisk V3 35K](https://img.shields.io/badge/tested%20on-Basilisk%20V3%2035K-44D62C)

Lightweight replacement for Razer Synapse's RGB engine on Windows. Keep designing your
profiles and Chroma Studio effects in Synapse, close it, and a 450 KB Rust daemon using
under 2 MB of private memory takes over: one RGB effect per on-board profile, switched with the profile button under the mouse,
with Synapse and all Razer services shut down.

> Independent project, **not affiliated with or endorsed by Razer Inc.** Razer, Synapse and
> Chroma are trademarks of Razer Inc.

## What nobody else does

DemonNapse is a **companion to Synapse, not another replacement for it**. As far as I could
find (September 2026), no other project does these three things:

1. **Synapse stays your editor.** Every alternative replaces Synapse with its own UI or config
   file, so you rebuild your lighting from scratch with whatever effects it offers. Here you
   keep designing in Chroma Studio, and the daemon imports the effects by reading Synapse's
   own log files. Nothing to redo, nothing new to learn.
2. **A Chroma Studio effect per on-board profile, with Synapse closed.** The mouse memory
   stores keys and DPI, never lighting, so any per-profile lighting Razer offers (Linked
   Games, "switch lighting when profile is switched") only works while Synapse is running.
   People have been [asking for this for years](https://insider.razer.com/razer-chroma-rgb-21/different-lighting-for-each-profile-50709).
   The daemon detects the profile button under the mouse and switches effect by itself.
3. **Automatic hand-over.** Open Synapse and the daemon starts the Razer services, releases
   the mouse and waits. Close Synapse and it re-imports your changes, stops the services and
   takes the mouse back. You never touch a terminal or a config file.

## Why

Synapse 4 cannot bind a Chroma Studio effect to an on-board profile (confirmed by Razer
support: the mouse memory stores keys and DPI, never lighting), and the app plus its 14
background services sit at 600–800 MB of RAM. DemonNapse fixes both.

| | Synapse 4 + services | DemonNapse |
|---|---|---|
| RAM (private working set) | ~650 MB + 168 MB | 1.5–2 MB |
| CPU (60 s with music playing and the Audio Meter driving the LEDs) | — | ~0.3 % of one core |
| Chroma Studio effect per on-board profile | no | yes |

The daemon was first written in Python (kept in `python/` as the reference implementation:
15 MB of private memory, ~0.7 % of one core) and then ported to Rust. `tools/parity.py`
checks that both render the same frames, `tools/bench.py` measures a running daemon.

## How it compares

Other people noticed how heavy Synapse is, and their projects are good at things DemonNapse
does not try to do. Pick the one that matches what you want:

| Project | What it is | Imports your Synapse / Chroma Studio setup | Effect per on-board profile | Devices |
|---|---|---|---|---|
| [OpenRGB](https://openrgb.org) | Vendor-neutral RGB control, all brands, all OSes | no | no | hundreds, all brands |
| [openrazer-win](https://github.com/Ar4ikov/openrazer-win) | Windows port of OpenRazer: daemon, CLI and Python library | no | no | 260+ Razer devices |
| [opsrzr](https://github.com/atv57/opsrzr) | Tray app: preset RGB, DPI, polling rate, battery | no | no | a few Razer devices |
| [razer-win-cli](https://github.com/FriskIsGit/razer-win-cli) | One-shot CLI, no daemon | no | no | a few Razer devices |
| [synapsekiller](https://github.com/nxroot/synapsekiller) | Starts Synapse, then kills its services after a delay | keeps what Synapse applied | no (no lighting engine) | any |
| **DemonNapse** | Synapse companion daemon | **yes, from Synapse's logs** | **yes** | one mouse verified |

If you want to get rid of Synapse entirely, or you own many devices, use OpenRGB or
openrazer-win: their device support is far beyond this project. DemonNapse is for people who
like Chroma Studio and only want it to stop running all day.

## How it works

1. **Synapse stays your editor.** Profiles, key bindings, DPI stages and Chroma Studio effects
   are designed in Synapse/Chroma as usual. Synapse writes all of it to its own log files.
2. **The daemon reads those logs** (`src/extract.rs`): the layers of every Chroma Studio
   effect, whatever its type, and the on-board slot of every profile.
3. **Profiles are matched to effects by name**: profile `CS2` → effect `Chroma Effect CS2`,
   `DOTA 2` → `Chroma Effect DOTA`. Words in common win; `effect_overrides` in `config.json`
   forces a match when the names differ.
4. **The mouse tells the daemon which profile is active** (vendor HID register `0x05/0x84`,
   plus the DPI/scroll events the firmware sends on every switch). The daemon renders the
   matching effect frame by frame over HID (`0x0F/0x03`, per-LED) — no Razer software needed.
   The "Hybrid Storage" slot (white indicator) is skipped automatically by jumping to the
   next configured slot (`0x05/0x04`), so a 3-profile setup is 3 clean clicks.
5. **Lifecycle.** At start the daemon kills leftover Razer processes, stops the Razer
   services and puts the mouse back in normal device mode (a Synapse that was killed leaves
   it in driver mode, where the profile button no longer switches slot). When you launch
   Synapse it notices within ~1.5 s, starts the services, releases the mouse and waits. When you close Synapse it re-reads effects and profiles, stops the
   services and takes the mouse back. You never touch a terminal.

Supported layers: **Static**, **Spectrum Cycling**, **Wave**, **Wheel**, **Tidal**, **Ripple**,
**Starlight**, **Breathing**, **Fire**, **Audio Meter** (system output peak via
`IAudioMeterInformation`, no audio capture) and **Reactive** (mouse button flash), stacked in
the same order as in Chroma Studio. When an effect has an Audio Meter, it has priority: the
other layers only show after `audio_idle_delay` seconds of silence. Playback options are honoured
(start on a click, stop after N cycles or on the next click), and so are layer groups. Wheel and
Tidal use the center point you set in Chroma Studio. Not supported yet: **Ambient Awareness** (it
needs screen capture) and **Chroma Generate** (AI/pattern generator).

Razer does not document what "speed 15" or "width 100" mean, so timings are calibrated by eye
(constants at the top of `src/layers.rs`). To compare an effect with Synapse without assigning
it to a profile: `python tools/preview.py "Chroma Effect X" 30` (it uses the Python renderer).

## Requirements

- Windows 10/11
- Razer Synapse 4 installed (used only as the editor; it is not running while the daemon works)
- `demonnapse.exe`: build it with `cargo build --release` ([Rust toolchain](https://rustup.rs),
  MSVC target, no C compiler needed), or put a prebuilt one next to `install.cmd`
- A supported mouse (see below)

Python is only needed for the scripts in `tools/` and for the reference daemon in `python/`.

## Install

1. In Synapse: put your profiles in on-board memory slots, name a Chroma Studio effect with a
   word in common with each profile, **modify a layer after renaming** (Synapse only logs the
   new name when something changes), then quit Synapse completely (tray → Exit).
2. `target\release\demonnapse.exe extract` — check that every effect and profile is listed.
3. Right-click `install.cmd` → **Run as administrator**. It looks for `demonnapse.exe` next to
   itself, then in `target\release`. It creates a scheduled task at logon with elevated rights
   (needed to stop/start Razer services), removes Synapse from Windows autostart, sets the Razer services to manual start and launches the daemon.

Log: `demonnapse.log` next to `config.json`. If the mouse is unplugged or a file is missing the
daemon logs the error once and retries every 5 seconds. Debug run, from a terminal, with every
HID event logged: stop the task (`schtasks /End /TN DemonNapse`), then `demonnapse.exe -v`.

Other commands, used to check the daemon piece by piece: `probe` (firmware, active slot, device
mode), `profiles` (profile → effect), `audio` (output peak), `synapse` (is Synapse open, state
of the Razer services), `render` (frames of an effect as hex).

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
probably work with a different `pid`, `leds` and `chroma_grid`; `tools/sniff.py` (Python, `pip install hidapi`) shows what
your mouse sends when you press the profile button. Reports and pull requests welcome.

## Things you should know before installing

- The daemon **runs elevated**, **kills `RazerAppEngine.exe`** and **stops the Razer Windows
  services** while it owns the mouse. Read `src/daemon.rs` and `src/synapse.rs` (they are
  short) if that worries you.
- It depends on the **format of Synapse 4's log files**, which Razer can change at any time.
  If an update breaks extraction, the daemon keeps the last known effects.
- It uses only user-mode APIs (`hidapi` feature reports, `GetAsyncKeyState`, WASAPI meter),
  the same approach as OpenRGB/OpenRazer. Not tested against anti-cheat systems.
- Every Chroma Studio layer is imported; all of them are rendered except Ambient Awareness,
  which is ignored with a warning. Speeds and widths may differ slightly from Synapse.

## Acknowledgements

The HID packet format comes from the
[OpenRazer](https://github.com/openrazer/openrazer) reverse-engineering wiki. The profile
registers (`0x05/0x80..0x84`, `0x05/0x04`) were found by probing.

## License

MIT.
