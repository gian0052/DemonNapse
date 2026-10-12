# DemonNapse

![License: GPL-3.0-or-later](https://img.shields.io/badge/license-GPL--3.0--or--later-green)
![Rust](https://img.shields.io/badge/rust-single%20587%20KB%20exe-orange)
![Platform: Windows 10/11](https://img.shields.io/badge/platform-Windows%2010%2F11-0078D6)
![Tested on: Basilisk V3 35K](https://img.shields.io/badge/tested%20on-Basilisk%20V3%2035K-44D62C)

**DemonNapse is a small Windows program that plays your Razer mouse's lighting effects
without keeping Synapse running.**

https://github.com/user-attachments/assets/45d69d0a-e1d3-46b5-bd68-252f78d5a65e

**[Download the latest release](../../releases/latest)** · Own a different Razer mouse?
[Try it in two minutes](#devices).

Synapse's editor is fine; what's heavy is keeping it running. So DemonNapse does not give you
a new editor to learn: it leaves the editing to Razer's own apps and takes over the playing.
Build your profiles in Synapse and your lighting in Chroma Studio, as you always did. When you
are done, close Synapse: DemonNapse (587 KB, under 2 MB of memory) takes its place and plays
your Chroma Studio effects, one per on-board profile, switched with the profile button. Want
to change something? Open Synapse again, edit, close it: DemonNapse picks up the new version
by itself.

Your keys and DPI keep working too, because they live in the mouse's on-board memory. No
`RazerAppEngine`, no Razer services in the background, no more falling back to spectrum
cycling.

> Independent project, **not affiliated with or endorsed by Razer Inc.** Razer, Synapse and
> Chroma are trademarks of Razer Inc., used only to identify compatible hardware and software.

## What it does

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

## How it compares

As far as I could find (October 2026), no other project does the first three:

| Project | What it is | Imports your Chroma Studio effects | Effect per on-board profile |
|---|---|---|---|
| [OpenRGB](https://openrgb.org) | Vendor-neutral RGB control, all brands | no | no |
| [openrazer-win](https://github.com/Ar4ikov/openrazer-win) | Windows port of OpenRazer, 260+ Razer devices | no | no |
| [neuron](https://github.com/worflor/neuron) | Tray app meant to remove Synapse, with its own effects | a Synapse 3 export, not Chroma Studio | no |
| [Artemis](https://github.com/Artemis-RGB/Artemis) / [Aurora](https://github.com/Aurora-RGB/Aurora) | Layered RGB for all brands | no, and Synapse must be running | no |
| **DemonNapse** | Synapse companion | **yes, from Synapse's logs** | **yes** |

If you want to get rid of Synapse entirely, or you own many devices, use OpenRGB or
openrazer-win: their device support is far beyond this project. DemonNapse is for people who
like Chroma Studio and only want it to stop running all day. [More projects compared](docs/comparison.md).

## How light

| | Synapse 4 + services | DemonNapse |
|---|---|---|
| RAM (private working set) | ~600 MB in the tray, ~940 MB with the window open | 1.5–2 MB |
| CPU | — | ~0.1 % of one core, ~0.3 % with the Audio Meter following music |
| Executable | — | one 587 KB file |

<sub>Measured on the author's PC (Windows 11, Basilisk V3 35K, Synapse 4) on 11 October 2026,
all Razer processes summed. One machine, not a benchmark; yours will differ.</sub>

One layer is heavy: **Ambient Awareness** reads the screen, which costs about 55 MB more while
an effect that uses it is on the mouse, and nothing otherwise.

## Install

You need Windows 10/11 and Razer Synapse 4 installed (it stays your editor; it is not running
while the daemon works).

1. Download the zip from the [latest release](../../releases/latest) and extract it anywhere.
2. In Synapse: put your profiles in on-board memory slots and give each Chroma Studio effect
   a word in common with its profile (for example: if you have a profile called `Gaming`,
   name the effect `Chroma Effect Gaming`). **Change
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
`demonnapse.exe check`. It opens its own window and in about two minutes tells you which
model it found (from the tables in [Razer mice](docs/razer-mice.md)), opens the mouse, reads
its profiles, asks you to press the profile button and lights the LEDs one at a time, asking
you whether each one lit. No `config.json` is needed and nothing is written to the mouse
memory. It saves the report as `demonnapse-check-XXXX.txt` next to `demonnapse.exe`: attach it
to a [device report](../../issues/new?template=device_report.yml), or to a
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
- [How it compares](docs/comparison.md): the full table of the projects I looked at.
- [Razer mice](docs/razer-mice.md): LED count, layout, colour command and transaction id of
  every Razer mouse with per-LED lighting, for checking support on a new model.

## Acknowledgements

The HID packet format comes from the
[OpenRazer](https://github.com/openrazer/openrazer) reverse-engineering wiki. The commands
for on-board profiles (`0x05/0x80..0x84`, `0x05/0x04`) were not documented: I found them by
trying commands on the mouse one at a time.

## License

**GPL-3.0-or-later.** Copyright (C) 2026 DemonNapse contributors.

DemonNapse is free software: you can redistribute it and/or modify it under the terms of the
GNU General Public License as published by the Free Software Foundation, either version 3 of
the License, or (at your option) any later version. It is distributed in the hope that it will
be useful, but WITHOUT ANY WARRANTY; see [LICENSE](LICENSE) for the full text.
