# DemonNapse

![License: MIT](https://img.shields.io/badge/license-MIT-green)
![Rust](https://img.shields.io/badge/rust-single%20490%20KB%20exe-orange)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6)
![Tested on: Basilisk V3 35K](https://img.shields.io/badge/tested%20on-Basilisk%20V3%2035K-44D62C)

Lightweight replacement for Razer Synapse's RGB engine on Windows. Keep designing your
profiles and Chroma Studio effects in Synapse, close it, and a 490 KB Rust daemon using
under 2 MB of private memory takes over: one RGB effect per on-board profile, switched with the profile button under the mouse,
with Synapse and all Razer services shut down.

> Independent project, **not affiliated with or endorsed by Razer Inc.** Razer, Synapse and
> Chroma are trademarks of Razer Inc.

## What nobody else does

DemonNapse is a **companion to Synapse, not another replacement for it**. As far as I could
find (September 2026), no other project does these four things:

1. **Your Chroma Studio effects run with Synapse closed.** Every alternative replaces Synapse
   with its own UI or config file, so you rebuild your lighting from scratch with whatever
   effects it offers; the tools built on Razer's Chroma SDK (Artemis, Aurora) need Synapse
   running. Here you keep designing in Chroma Studio, the daemon imports the effects by reading
   Synapse's own log files and renders every layer type itself, measured frame by frame
   against Razer's engine. Nothing to redo, nothing new to learn.
2. **A Chroma Studio effect per on-board profile, with Synapse closed.** The mouse memory
   stores keys and DPI, never lighting, so any per-profile lighting Razer offers (Linked
   Games, "switch lighting when profile is switched") only works while Synapse is running.
   People have been [asking for this for years](https://insider.razer.com/razer-chroma-rgb-21/different-lighting-for-each-profile-50709).
   The daemon detects the profile button under the mouse and switches effect by itself.
3. **Automatic hand-over.** Open Synapse and the daemon starts the Razer services, releases
   the mouse and waits. Close Synapse and it re-imports your changes, stops the services and
   takes the mouse back. You never touch a terminal or a config file.
4. **About 2 MB of memory.** A single 490 KB executable using 1.5–2 MB of private memory and
   ~0.1 % of one core. Synapse and its services sit at 600–800 MB; the lightest alternatives
   declare 14–40 MB.

## Why

Synapse 4 cannot bind a Chroma Studio effect to an on-board profile (confirmed by Razer
support: the mouse memory stores keys and DPI, never lighting), and the app plus its 14
background services sit at 600–800 MB of RAM. DemonNapse fixes both.

| | Synapse 4 + services | DemonNapse |
|---|---|---|
| RAM (private working set) | ~650 MB + 168 MB | 1.5–2 MB |
| CPU (60 s with music playing and the Audio Meter driving the LEDs) | — | ~0.3 % of one core |
| Chroma Studio effect per on-board profile | no | yes |
| **With an Ambient layer active** (screen capture) | — | **~55 MB** and ~0.3 % more of one core |

> **Ambient Awareness is the one heavy layer.** Reading the screen needs the GPU driver, which
> adds about 55 MB of private memory while an effect with an Ambient layer is on the mouse. The
> daemon does not even load the capture code otherwise: `d3d11.dll` and `dxgi.dll` are
> delay-loaded, so with no profile using Ambient they are never mapped, and switching to a
> profile without it closes the capture again. If you want the daemon at its lightest, leave
> Ambient out of your effects.

The daemon was first written in Python (kept in `python/` as the reference implementation:
15 MB of private memory, ~0.7 % of one core) and then ported to Rust. `tools/parity.py`
checks that both render the same frames, except layers with random colours and the layers
redone in Rust on Razer's measurements (Wave, Wheel, Tidal, Ripple, Fire, Starlight, Ambient),
which `cargo test` covers instead; `tools/bench.py` measures a running daemon.

## How it compares

Other people noticed how heavy Synapse is, and their projects are good at things DemonNapse
does not try to do. Pick the one that matches what you want:

| Project | What it is | Imports your Synapse / Chroma Studio setup | Effect per on-board profile | Devices |
|---|---|---|---|---|
| [OpenRGB](https://openrgb.org) | Vendor-neutral RGB control, all brands, all OSes | no | no | hundreds, all brands |
| [openrazer-win](https://github.com/Ar4ikov/openrazer-win) | Windows port of OpenRazer: daemon, CLI and Python library | no | no | 260+ Razer devices |
| [opsrzr](https://github.com/atv57/opsrzr) | Tray app: preset RGB, DPI, polling rate, battery | no | no | a few Razer devices |
| [razer-win-cli](https://github.com/FriskIsGit/razer-win-cli) | One-shot CLI, no daemon | no | no | a few Razer devices |
| [snakecharmer](https://github.com/asavs/snakecharmer) | Static and hardware effects per zone, plus a toolkit to reverse-engineer new mice | no | no | a few Razer devices |
| [neuron](https://github.com/worflor/neuron) | Tray app and CLI meant to remove Synapse: own effects, remapping, writes to on-board memory | a Synapse 3 export, not Chroma Studio | no | a few Razer devices |
| [Artemis](https://github.com/Artemis-RGB/Artemis) / [Aurora](https://github.com/Aurora-RGB/Aurora) | Layered and game-aware RGB for all brands | no, and they drive Razer devices through the Chroma SDK, so Synapse must run | no | many, all brands |
| [synapsekiller](https://github.com/nxroot/synapsekiller) | Starts Synapse, then kills its services after a delay | keeps what Synapse applied | no (no lighting engine) | any |
| **DemonNapse** | Synapse companion daemon | **yes, from Synapse's logs** | **yes** | one mouse verified |

If you want to get rid of Synapse entirely, or you own many devices, use OpenRGB or
openrazer-win: their device support is far beyond this project. DemonNapse is for people who
like Chroma Studio and only want it to stop running all day.

## How it works

1. **Synapse stays your editor.** Profiles, key bindings, DPI stages and Chroma Studio effects
   are designed in Synapse/Chroma as usual. Synapse writes all of it to its own log files.
2. **The daemon reads those logs** (`src/extract.rs`): the layers of every Chroma Studio
   effect, whatever its type, the on-board slot of every profile and the mouse's LED map (which
   Chroma Studio grid cell is which physical LED).
3. **Profiles are matched to effects by name**: profile `CS2` → effect `Chroma Effect CS2`,
   `DOTA 2` → `Chroma Effect DOTA`. Names are compared word by word (letters and digits only,
   case ignored, a leading `Chroma Effect` does not count); the effect with the most words in
   common wins, and on a tie the first one in Synapse's logs. `effect_overrides` in
   `config.json` forces a match when the names differ. A profile with no matching effect keeps
   its RGB **off** (the log says so). Effects can have any name: `Chroma Effect …` is only
   Synapse's default, a renamed effect is read the same way.
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
the same order as in Chroma Studio. Like Razer's engine, the Audio Meter paints its whole region
with one colour of the gradient (position = output peak × boost, auto boost included) and lets
the layers below show through when the level falls back to zero. Playback options are honoured
(start on a click, stop after N cycles or on the next click), and so are layer groups. Wheel and
Tidal use the center point you set in Chroma Studio. **Ambient Awareness** reads the colours of
the primary screen like Razer's engine (one pixel per grid cell of the region, the chosen screen
area, the same blur, every 40 ms) through DXGI Desktop Duplication, and only while the active
effect has an Ambient layer: then the daemon uses about 55 MB more private memory (the GPU
driver) and ~0.3 % more of one core. With any other effect the capture libraries are not even
loaded and the cost is zero (see the note under [Why](#why)).
**Chroma Generate** (AI/pattern generator from images and videos) is not supported and will not
be: it is the opposite of lightweight.

Razer does not document what "speed 15" or "width 100" mean: timings, shapes and the Audio
Meter were measured on the Chroma Studio preview, which shows the frames of Razer's own engine
(constants and notes at the top of `src/layers.rs`, `src/effect.rs` and `src/ambient.rs`).

## Requirements

- Windows 10/11
- Razer Synapse 4 installed (used only as the editor; it is not running while the daemon works)
- `demonnapse.exe`: build it with `cargo build --release` ([Rust toolchain](https://rustup.rs),
  MSVC target, no C compiler needed), or put a prebuilt one next to `install.cmd`
- A supported mouse (see below)

Python is only needed for the scripts in `tools/` and for the reference daemon in `python/`
(`pip install -r python/requirements.txt`). Besides `parity.py`, `bench.py` and `sniff.py`:
`tools/preview.py` shows a Chroma Studio effect on the mouse without assigning it to a profile,
and `tools/synapse-inspect.mjs` (Node) reads Chroma Studio's options from a running Synapse
started with `--inspect`, the way the layers were measured.

## Install

1. In Synapse: put your profiles in on-board memory slots, name a Chroma Studio effect with a
   word in common with each profile, **modify a layer after renaming** (Synapse only logs the
   new name when something changes), then quit Synapse completely (tray → Exit).
2. Optional: `demonnapse.exe extract` (the one next to `install.cmd`, or
   `target\release\demonnapse.exe` if you built it) lists every effect and profile found in
   Synapse's logs. The daemon reads them by itself on its first start and again every time
   you close Synapse.
3. Right-click `install.cmd` → **Run as administrator**. It looks for `demonnapse.exe` next to
   itself, then in `target\release`. It creates a scheduled task at logon with elevated rights
   (needed to stop/start Razer services), removes Synapse from Windows autostart, sets the Razer services to manual start and launches the daemon.

Log: `demonnapse.log` next to `config.json`, one line per event as `YYYY-MM-DD HH:MM:SS message`.
It is never rotated; it grows by a few lines a day, delete it whenever you like. If the mouse is
unplugged or a file is missing the daemon logs the error once and retries every 5 seconds.
Debug run with every HID event logged: stop the task (`schtasks /End /TN DemonNapse`), then run
`demonnapse.exe -v` from a terminal **opened as administrator** (otherwise it cannot stop the
Razer services); from a terminal the log goes to the screen instead of the file.

Other commands, used to check the daemon piece by piece (stop the task first for the ones that
talk to the mouse, `probe`):

| command | what it does |
|---|---|
| `extract [--out FOLDER]` | effects and profiles from Synapse's logs, written next to `config.json` or in `FOLDER` |
| `profiles` | profile → matched effect, as JSON |
| `probe` | firmware, active slot and device mode of the mouse |
| `audio [seconds]` | output peak, 10 readings a second (default 3 s) |
| `synapse [--kill NAME.exe]` | is Synapse open, are we admin, state of the Razer services; `--kill` closes one process the way the daemon closes Synapse |
| `render "<effect>" <t0> <frames> <fps>` | frames of an effect from time `t0` (seconds), one line of hex colours per frame |

`render` options: `--layer NAME` only that layer (first region), `--all` the effect spread over
every LED, `--clicks S,S,...` mouse clicks at those seconds (Reactive and playback on click),
`--live` renders in real time, for Ambient. `demonnapse.exe` has no window of its own, so in a
terminal the prompt may come back before the output: press Enter once it has printed.

To go back exactly to how things were: `uninstall.cmd` as administrator. On the first install
`install.cmd` saves to `razer_backup.txt` the start type of the Razer services, Synapse's autostart
entry and whether that entry is disabled in Task Manager; `uninstall.cmd` puts back exactly those.
Without that file (installed with an older version) the services are set to automatic and Synapse
gets its standard, enabled autostart entry.

## Configuration (`config.json`)

The daemon looks for `config.json` next to `demonnapse.exe` or up to two folders above it (so
`target\release\demonnapse.exe` finds the one in the repository root), otherwise in the current
folder. The extracted files and the log are written in the same folder.

| key | meaning |
|---|---|
| `device.pid` | USB product id of your mouse |
| `device.leds` / `device.chroma_grid` | optional: LED count and Chroma Studio grid position `"row,col"` → physical LED index. Read from Synapse's logs (`synapse_device.json`) when missing; set them only to override |
| `effect_overrides` | `{"Profile name": "Chroma Effect X"}` when names have nothing in common |
| `skip_slots` | on-board slots to skip (default `[1]`, the Hybrid Storage slot) |
| `fps` | frames per second sent to the mouse (default 60) |
| `audio_gain` | multiplies the output peak before the Audio Meter (default 1, same as Razer) |

## Supported devices

Verified on the **Razer Basilisk V3 35K (wired, PID `0x00CB`)** only: control interface,
per-LED protocol, LED map and profile registers were all reverse-engineered and tested on that
mouse. Other Razer mice speaking the same protocol (most of them, see the OpenRazer wiki) will
probably work with a different `pid` (the LED count and map come from Synapse's logs); `tools/sniff.py` (Python, `pip install hidapi`) shows what
your mouse sends when you press the profile button. Reports and pull requests welcome.

## Things you should know before installing

- The daemon **runs elevated**, **kills `RazerAppEngine.exe`** and **stops the Razer Windows
  services** while it owns the mouse. Read `src/daemon.rs` and `src/synapse.rs` (they are
  short) if that worries you.
- It depends on the **format of Synapse 4's log files**, which Razer can change at any time.
  If an update breaks extraction, the daemon keeps the last known effects.
- It uses only user-mode APIs (`hidapi` feature reports, `GetAsyncKeyState`, WASAPI meter,
  DXGI Desktop Duplication for Ambient),
  the same approach as OpenRGB/OpenRazer. Not tested against anti-cheat systems.
- Every frame is also sent as a small UDP packet to `127.0.0.1:47311` (local only, it never
  leaves the PC), for an experimental desktop wallpaper that takes the mouse colours; nothing
  listens there unless you run it, and `tools/bench.py` uses it to count frames.
- Every Chroma Studio layer is imported; all of them are rendered except Chroma Generate,
  which is ignored with a warning.
- A "click" means a **mouse button** (left, right, middle, side buttons), never the keyboard,
  even where Chroma Studio says "on any key pressed".
- **Tidal started by a click** (playback start "on any key pressed") does not match Razer's
  engine yet: it starts and stops, but the timing of each run differs. A Tidal that always runs
  is rendered like Razer's.

## Acknowledgements

The HID packet format comes from the
[OpenRazer](https://github.com/openrazer/openrazer) reverse-engineering wiki. The profile
registers (`0x05/0x80..0x84`, `0x05/0x04`) were found by probing.

## License

MIT.
