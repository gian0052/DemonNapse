# How DemonNapse works

Back to the [README](../README.md).

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
   **Lights off without software:** press the profile button 5 times within 1.5 s and the
   LEDs go dark; do it again and they come back. The profile you started from is restored,
   so the burst does not leave you on another slot, and the lights stay off across a Synapse
   hand-over until you turn them back on. Count and time window are in `config.json`.
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
loaded and the cost is zero (see [How light](../README.md#how-light)).
**Chroma Generate** (AI/pattern generator from images and videos) is not supported and will not
be: it is the opposite of lightweight.

Razer does not document what "speed 15" or "width 100" mean: timings, shapes and the Audio
Meter were measured on the Chroma Studio preview, which shows the frames of Razer's own engine
(constants and notes at the top of `src/layers.rs`, `src/effect.rs` and `src/ambient.rs`).

## Ambient Awareness

**Ambient Awareness is the one heavy layer.** Reading the screen needs the GPU driver, which
adds about 55 MB of private memory while an effect with an Ambient layer is on the mouse. The
daemon does not even load the capture code otherwise: `d3d11.dll` and `dxgi.dll` are
delay-loaded, so with no profile using Ambient they are never mapped, and switching to a
profile without it closes the capture again. If you want the daemon at its lightest, leave
Ambient out of your effects.

## Where it comes from

The daemon was first written in Python (15 MB of private memory, ~0.7 % of one core) and then
ported to Rust; the Python version is in the git history up to release 1.1.0. `cargo test`
checks the layers against the measurements taken on Razer's engine.
