# DemonNapse

![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-green)
![Rust](https://img.shields.io/badge/rust-single%20526%20KB%20exe-orange)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6)
![Tested on: Basilisk V3 35K](https://img.shields.io/badge/tested%20on-Basilisk%20V3%2035K-44D62C)

Your Chroma Studio effects, **without Synapse running**, on Windows. Keep designing your
profiles and effects in Synapse, close it, and a 526 KB Rust daemon using under 2 MB of
private memory plays them on the mouse: one effect per on-board profile, switched with the
profile button under the mouse, with `RazerAppEngine` and every Razer service shut down. No
more falling back to spectrum cycling.

> Independent project, **not affiliated with or endorsed by Razer Inc.** Razer, Synapse and
> Chroma are trademarks of Razer Inc., used only to identify compatible hardware and software.

## What it does

DemonNapse is a **companion to Synapse, not another replacement for it**.

- **Plays your Chroma Studio effects with Synapse closed.** You keep designing in Chroma
  Studio; the daemon reads the effects from Synapse's own log files and renders them itself.
  Nothing to rebuild, nothing new to learn.
- **One effect per on-board profile.** Press the profile button under the mouse and the
  lighting changes with the profile. Razer only does this while Synapse is running, and people
  have been [asking for it for years](https://insider.razer.com/razer-chroma-rgb-21/different-lighting-for-each-profile-50709).
- **Hands the mouse over by itself.** Open Synapse and the daemon steps aside; close it and
  the daemon picks up your changes and takes the mouse back. No terminal, no config file.
- **Lights off from the mouse.** Press the profile button 5 times quickly and the LEDs go
  dark; do it again and they come back.

As far as I could find (October 2026), no other project does the first three: see
[how it compares](docs/comparison.md).

## How light

| | Synapse 4 + services | DemonNapse |
|---|---|---|
| RAM (private working set) | ~650 MB + 168 MB | 1.5–2 MB |
| CPU | — | ~0.1 % of one core, ~0.3 % with the Audio Meter following music |
| Executable | — | one 526 KB file |

<sub>Measured on the author's PC (Windows 11, Basilisk V3 35K, Synapse 4). One machine, not a
benchmark; yours will differ.</sub>

One layer is heavy: **Ambient Awareness** reads the screen, which costs about 55 MB more while
an effect that uses it is on the mouse, and nothing otherwise.

## Install

You need Windows 10/11 and Razer Synapse 4 installed (it stays your editor; it is not running
while the daemon works).

1. Download the zip from the [latest release](../../releases/latest) and extract it anywhere.
2. In Synapse: put your profiles in on-board memory slots and give each Chroma Studio effect
   a word in common with its profile (profile `CS2` → effect `Chroma Effect CS2`). **Change
   something in a layer after renaming** (Synapse only logs the new name when something
   changes), then quit Synapse completely (tray → Exit).
3. Right-click `install.cmd` → **Run as administrator**.

`install.cmd` creates a scheduled task that starts the daemon at logon, removes Synapse from
Windows autostart and sets the Razer services to manual start. `uninstall.cmd` (as
administrator) puts all of that back exactly as it was.

To build it yourself: `cargo build --release` ([Rust toolchain](https://rustup.rs), MSVC
target, no C compiler needed).

## Devices

| | |
|---|---|
| **Verified** | Razer Basilisk V3 35K, wired (PID `0x00CB`) |
| **Should work, not tried yet** | Other Razer mice with on-board profiles and per-LED lighting, wired or through their wireless dongle |
| **Planned** | Razer keyboards and other Chroma devices |

Only one mouse is verified because it is the one I own, not because the daemon is tied to it:
it finds the mouse among the connected Razer devices, reads its LED count and LED map from
Synapse's logs and detects the protocol variant by itself. Nothing in it is specific to a
cable, but nobody has run it on a wireless mouse yet. Today it drives one mouse; a Razer
keyboard plugged in next to it is left alone.

**Own a different Razer mouse? Two minutes of yours put it on the list.** Close Synapse (and
stop the daemon if it is installed: `schtasks /End /TN DemonNapse`), then run
`demonnapse.exe check`: in about 30 seconds it opens the mouse, reads its profiles, asks
you to press the profile button and lights the LEDs one at a time. Nothing is written to the
mouse memory. Then paste the output in a
[device report](../../issues/new?template=device_report.yml), or open a
[device support request](../../issues/new?template=device_support.yml) if `check` stops with
an error.

## Before you install

- The daemon **runs elevated**, **closes `RazerAppEngine.exe`** and **stops the Razer Windows
  services** while it owns the mouse. `src/daemon.rs` and `src/synapse.rs` are short, read
  them if that worries you.
- It depends on the **format of Synapse 4's log files**, which Razer can change at any time.
  If an update breaks the import, the daemon keeps the last effects it read.
- **No kernel driver, no input injection.** User-mode APIs only; it reads whether a mouse
  button is down and never sends keys or clicks. Not tested against anti-cheat systems.
- The executable is **not code-signed**. SmartScreen may warn on first run (More info → Run
  anyway); with Smart App Control turned on, Windows 11 blocks unsigned programs outright.
  Each release lists the SHA-256 of every file in `SHA256SUMS.txt`, or build it from source.
- Every frame is also sent as a UDP packet to `127.0.0.1:47311`, for an experimental desktop
  wallpaper. It never leaves the PC and nothing listens there unless you run it.
- Known limits: **Chroma Generate** is not rendered; a "click" means a mouse button, never the
  keyboard; a **Tidal** layer started by a click does not follow Razer's timing yet.

## More

- [How it works](docs/how-it-works.md): how effects are read from the logs and matched to
  profiles, the supported layers, the hand-over with Synapse.
- [Reference](docs/reference.md): `config.json`, the other commands, the log, uninstalling.
- [How it compares](docs/comparison.md) with OpenRGB, openrazer-win and the others.

## Acknowledgements

The HID packet format comes from the
[OpenRazer](https://github.com/openrazer/openrazer) reverse-engineering wiki. The profile
registers (`0x05/0x80..0x84`, `0x05/0x04`) were found by probing.

## License

Copyright (C) 2026 DemonNapse contributors.

DemonNapse is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of
the License, or (at your option) any later version. It is distributed in the hope that it will
be useful, but WITHOUT ANY WARRANTY; see [LICENSE](LICENSE) for the full text.
