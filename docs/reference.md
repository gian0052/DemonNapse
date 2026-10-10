# Reference: commands, configuration, log

Back to the [README](../README.md).

## Log and debug run

Log: `demonnapse.log` next to `config.json`, one line per event as `YYYY-MM-DD HH:MM:SS message`.
It is never rotated; it grows by a few lines a day, delete it whenever you like. If the mouse is
unplugged or a file is missing the daemon logs the error once and retries every 5 seconds.
Debug run with every HID event logged: stop the task (`schtasks /End /TN DemonNapse`), then run
`demonnapse.exe -v` from a terminal **opened as administrator** (otherwise it cannot stop the
Razer services); from a terminal the log goes to the screen instead of the file.

## Commands

Other commands, used to check the daemon piece by piece (stop the task first for the ones that
talk to the mouse, `probe`):

| command | what it does |
|---|---|
| `check [PID]` | tries a Razer mouse step by step (see [Devices](../README.md#devices)) and prints a report |
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

## Uninstall

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
| `device.pid` | optional: USB product id of the mouse, e.g. `"0x00CB"`. When missing the daemon uses the connected Razer device that Synapse lists as a mouse (never a keyboard or headset), or the only Razer device connected if Synapse does not know its type; with several candidates it asks you to set it |
| `device.leds` / `device.chroma_grid` | optional: LED count and Chroma Studio grid position `"row,col"` → physical LED index. Read from Synapse's logs (`synapse_device.json`) when missing; set them only to override |
| `effect_overrides` | `{"Profile name": "Chroma Effect X"}` when names have nothing in common |
| `skip_slots` | on-board slots to skip (default `[1]`, the Hybrid Storage slot) |
| `fps` | frames per second sent to the mouse (default 60) |
| `audio_gain` | multiplies the output peak before the Audio Meter (default 1, same as Razer) |
| `lights_toggle_clicks` | profile button presses that turn the lights off and on again (default 5, `0` disables it) |
| `lights_toggle_seconds` | time window for those presses, in seconds (default 1.5) |

## Tools

Python is only needed for the scripts in `tools/` and for the reference daemon in `python/`
(`pip install -r python/requirements.txt`). Besides `parity.py`, `bench.py` and `sniff.py`:
`tools/preview.py` shows a Chroma Studio effect on the mouse without assigning it to a profile,
and `tools/synapse-inspect.mjs` (Node) reads Chroma Studio's options from a running Synapse
started with `--inspect`, the way the layers were measured.

`tools/sniff.py` (Python, `pip install hidapi`) shows everything the mouse sends.
