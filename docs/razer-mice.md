# Razer mice: LEDs per model

Back to the [README](../README.md). Data checked on 2026-10-12.

What every Razer mouse needs from DemonNapse: how many LEDs it has, how they are laid out, which
command sets them and which transaction id it answers to. The table below lists every mouse with
per-LED lighting known to OpenRGB, so a report from a new model can be checked against it and the
`check` command can be designed around real hardware instead of guesses.

Sources, facts only (IDs, counts, zone names), no code copied:

- OpenRGB `Controllers/RazerController/RazerDevices.cpp` and `RazerDevices.h`, commit
  `feab05b6` (2026-10-12): name, PID, matrix type, transaction id, rows, columns, zones.
- OpenRazer `daemon/openrazer_daemon/hardware/mouse.py`, commit `477f4d6d` (2026-10-05): used to
  find the mice that have no per-LED matrix.

## What the data says

- **Every mouse is a single row of LEDs.** The most is 21 (Diamondback Chroma, an old model); among
  recent ones 20 (Mamba Elite), 14 (Basilisk Ultimate), 13 (Basilisk V3 Pro and Pro 35K). One
  `0x0F/0x03` report holds up to 25 LEDs, so a mouse always fits in one report on row 0: no
  per-row or multi-report sending is needed for mice (keyboards are another matter).
- **53 of 63 entries use the extended matrix command `0x0F/0x03`**, the one DemonNapse sends. The
  other 10 are models from about 2012 to 2016 (Mamba 2012/2015/Tournament Edition, Diamondback
  Chroma, Naga Chroma, Naga Hex V2, Naga Epic Chroma, DeathAdder Chroma) and use older commands.
- **Transaction id is `0x1F` or `0x3F`.** DemonNapse tries `0x1F`, `0x3F`, `0xFF` in that order
  and keeps the one the mouse answers to, so both are covered.
- **Wired, wireless and Bluetooth have different PIDs** (Basilisk V3 Pro: `0x00AA` wired,
  `0x00AB` wireless, `0x00AC` Bluetooth). A report must say how the mouse was connected.
- **Many recent mice have no per-LED lighting at all** (list below). DemonNapse has nothing to
  play on them; `check` should say so instead of failing.
- **Nagas:** Naga Pro, Pro V3, Trinity, Classic, Left Handed have 3 LEDs (logo, scroll wheel,
  side numpad); Naga Pro V2 has 2 (logo, numpad); Naga X is 1x2 in OpenRazer; the Naga V2
  HyperSpeed has no per-LED matrix.
- **Not in these tables:** on-board profiles. Neither project documents which mice have them or
  how the profile commands (`0x05/0x80..0x84`, `0x05/0x04`) behave; that still has to come from
  reports of real mice.

## What the mouse says about itself

From version 1.3.0 `check` asks the mouse for its lighting zones with the read command
`0x0F/0x80`. The answer is five bytes per zone, `zone 19 03 rows columns`, in the order of the
LED indices. The Basilisk V3 35K answers `04 19 03 01 01`, `01 19 03 01 01`, `0A 19 03 01 09`:
logo 1, scroll wheel 1, strip 9, which is the layout verified by eye below. Zone ids seen so
far: `01` scroll wheel, `04` logo, `0A` strip, `05` the keys of a keyboard (several rows). When
a mouse answers, `check` uses that count before the tables on this page; the tables stay as
the fallback for mice that do not answer.

## Layouts verified by eye

Where a mouse was walked with `check` (one LED at a time, the owner saying where each one is). LED
numbers are the indices of the `0x0F/0x03` report, from 0.

**Razer Basilisk V3 35K** (`0x00CB`, 11 LEDs, three runs on 2026-10-12, same answers each time):

| LED | Where |
|---|---|
| 0 | logo |
| 1 | scroll wheel |
| 2, 3 | left side, front half, front to back |
| 4, 5, 6 | left side, rear half, front to back |
| 7, 8 | right side, rear half, back to front |
| 9, 10 | right side, front half, back to front |

So the strip goes round the mouse from the front of the left side to the front of the right side,
five LEDs on the left (the thumb side, which is longer) and four on the right. OpenRGB's zone order
"Logo 1, Scroll Wheel 1, LED Strip 9" is the index order on this mouse. Synapse's `LedMatrix` for
it is a top view: left column = LEDs 2..6 top to bottom, right column = LEDs 10..7 top to bottom,
wheel top centre, logo bottom centre. The Basilisk V3 (`0x0099`) has the same zones in OpenRGB and
very likely the same order, not verified.

## Mice with per-LED lighting (OpenRGB)

Sorted by LED count. Zones are listed in OpenRGB's order with their LED count; that order is not
proven to be the LED index order, Synapse's `LedMatrix` is what DemonNapse uses.

| Model | PID | LEDs | Rows x cols | Zones | Colour command | Transaction id |
|---|---|---|---|---|---|---|
| Razer Diamondback Chroma | `0x004C` | 21 | 1x21 | LED Strip 19, Logo 1, Scroll Wheel 1 | linear (old) | `0x3F` |
| Razer Mamba Elite | `0x006C` | 20 | 1x20 | Scroll Wheel 1, Logo 1, Left LED Strip 9, Right LED Strip 9 | `0F/03` | `0x1F` |
| Razer Lancehead 2017 (Wired) | `0x0059` | 16 | 1x16 | Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer Lancehead 2017 (Wireless) | `0x005A` | 16 | 1x16 | Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer Lancehead 2019 (Wired) | `0x0070` | 16 | 1x16 | Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer Lancehead 2019 (Wireless) | `0x006F` | 16 | 1x16 | Right LED Strip 7, Left LED Strip 7, Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer Lancehead Tournament Edition | `0x0060` | 16 | 1x16 | Scroll Wheel 1, Logo 1, Right LED Strip 7, Left LED Strip 7 | `0F/03` | `0x3F` |
| Razer Mamba Tournament Edition | `0x0046` | 16 | 1x16 | Left LED Strip 7, Right LED Strip 7, Logo 1, Scroll Wheel 1 | linear (old) | `0x3F` |
| Razer Mamba (Wireless) | `0x0045` | 15 | 1x15 | Chroma Zone 15 | linear (old) | `0x3F` |
| Razer Mamba 2012 (Wired) | `0x0024` | 15 | 1x15 | Scroll Wheel 1 | custom (old) | `0x3F` |
| Razer Mamba 2012 (Wireless) | `0x0025` | 15 | 1x15 | Scroll Wheel 1 | custom (old) | `0x3F` |
| Razer Mamba 2015 (Wired) | `0x0044` | 15 | 1x15 | Chroma Zone 15 | linear (old) | `0x3F` |
| Razer Basilisk Ultimate | `0x0086` | 14 | 1x14 | Scroll Wheel 1, Logo 1, Left LED Strip 8, Right LED Strip 4 | `0F/03` | `0x1F` |
| Razer Basilisk Ultimate (Wireless) | `0x0088` | 14 | 1x14 | Scroll Wheel 1, Logo 1, Left LED Strip 8, Right LED Strip 4 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro (Bluetooth) | `0x00AC` | 13 | 1x13 | Scroll Wheel 1, Logo 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro (Wired) | `0x00AA` | 13 | 1x13 | Scroll Wheel 1, Logo 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro (Wireless) | `0x00AB` | 13 | 1x13 | Scroll Wheel 1, Logo 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro 35K (Wired) | `0x00CC` | 13 | 1x13 | Logo 1, Scroll Wheel 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro 35K (Wireless) | `0x00CD` | 13 | 1x13 | Logo 1, Scroll Wheel 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro 35K Phantom Green Edition (Bluetooth) | `0x00D8` | 13 | 1x13 | Scroll Wheel 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro 35K Phantom Green Edition (Wired) | `0x00D6` | 13 | 1x13 | Scroll Wheel 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 Pro 35K Phantom Green Edition (Wireless) | `0x00D7` | 13 | 1x13 | Scroll Wheel 1, LED Strip 11 | `0F/03` | `0x1F` |
| Razer Basilisk V3 | `0x0099` | 11 | 1x11 | Logo 1, Scroll Wheel 1, LED Strip 9 | `0F/03` | `0x1F` |
| Razer Basilisk V3 35K | `0x00CB` | 11 | 1x11 | Logo 1, Scroll Wheel 1, LED Strip 9 | `0F/03` | `0x1F` |
| Razer Cobra Pro (Wired) | `0x00AF` | 11 | 1x11 | Logo 1, Scroll Wheel 1, Underglow 9 | `0F/03` | `0x1F` |
| Razer Cobra Pro (Wireless) | `0x00B0` | 11 | 1x11 | Logo 1, Scroll Wheel 1, Underglow 9 | `0F/03` | `0x1F` |
| Razer Mouse Bungee V3 Chroma | `0x0F1D` | 8 | 1x8 | LED Strip 8 | `0F/03` | `0x1F` |
| Razer Naga Chroma | `0x0053` | 3 | 1x3 | Scroll Wheel 1, Logo 1, Numpad 1 | linear (old) | `0x3F` |
| Razer Naga Classic | `0x0093` | 3 | 1x3 | Logo 1, Scroll Wheel 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Naga Hex V2 | `0x0050` | 3 | 1x3 | Logo 1, Scroll Wheel 1, Numpad 1 | standard (old) | `0x3F` |
| Razer Naga Left Handed | `0x008D` | 3 | 1x3 | Logo 1, Scroll Wheel 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Naga Pro (Wired) | `0x008F` | 3 | 1x3 | Scroll Wheel 1, Logo 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Naga Pro (Wireless) | `0x0090` | 3 | 1x3 | Scroll Wheel 1, Logo 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Naga Pro V3 (Wired) | `0x00E7` | 3 | 1x3 | Logo 1, Scroll Wheel 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Naga Pro V3 (Wireless) | `0x00E8` | 3 | 1x3 | Logo 1, Scroll Wheel 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Naga Trinity | `0x0067` | 3 | 1x3 | Scroll Wheel 1, Logo 1, Numpad 1 | `0F/03` | `0x3F` |
| Razer Basilisk | `0x0064` | 2 | 1x2 | Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer Basilisk V2 | `0x0085` | 2 | 1x2 | Scroll Wheel 1, Logo 1 | `0F/03` | `0x3F` |
| Razer DeathAdder Chroma | `0x0043` | 2 | 1x2 | Logo 1, Scroll Wheel 1 | custom (old) | `0x1F` |
| Razer DeathAdder Elite | `0x005C` | 2 | 1x2 | Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer DeathAdder Essential | `0x006E` | 2 | 1x2 | Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer DeathAdder Essential (White Edition) | `0x0071` | 2 | 1x2 | Logo 1, Scroll Wheel 1 | `0F/03` | `0x3F` |
| Razer DeathAdder Essential V2 | `0x0098` | 2 | 1x2 | Logo 1, Scroll Wheel 1 | `0F/03` | `0x1F` |
| Razer DeathAdder V2 | `0x0084` | 2 | 1x2 | Scroll Wheel 1, Logo 1 | `0F/03` | `0x1F` |
| Razer Mamba 2018 (Wired) | `0x0073` | 2 | 1x2 | Scroll Wheel Zone 1, Logo Zone 1 | `0F/03` | `0x3F` |
| Razer Mamba 2018 (Wireless) | `0x0072` | 2 | 1x2 | Scroll Wheel Zone 1, Logo Zone 1 | `0F/03` | `0x3F` |
| Razer Mamba Hyperflux (Wired) | `0x0069` | 2 | 1x2 | Scroll Wheel 1, Logo 1 | `0F/03` | `0x3F` |
| Razer Naga Epic Chroma | `0x003E` | 2 | 1x2 | Scroll Wheel 1, Numpad 1 | custom (old) | `0x1F` |
| Razer Naga Pro V2 (Wired) | `0x00A7` | 2 | 1x2 | Logo 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Naga Pro V2 (Wireless) | `0x00A8` | 2 | 1x2 | Logo 1, Numpad 1 | `0F/03` | `0x1F` |
| Razer Abyssus Elite DVa Edition | `0x006A` | 1 | 1x1 | Logo 1 | `0F/03` | `0x3F` |
| Razer Abyssus Essential | `0x006B` | 1 | 1x1 | Logo 1 | `0F/03` | `0x3F` |
| Razer Basilisk Essential | `0x0065` | 1 | 1x1 | Logo 1 | `0F/03` | `0x3F` |
| Razer Basilisk V3 X HyperSpeed | `0x00B9` | 1 | 1x1 | Scroll Wheel 1 | `0F/03` | `0x1F` |
| Razer Cobra | `0x00A3` | 1 | 1x1 | Logo 1 | `0F/03` | `0x1F` |
| Razer DeathAdder V2 (Wired) | `0x007C` | 1 | 1x1 | Logo 1 | `0F/03` | `0x3F` |
| Razer DeathAdder V2 (Wireless) | `0x007D` | 1 | 1x1 | Logo 1 | `0F/03` | `0x3F` |
| Razer DeathAdder V2 Mini | `0x008C` | 1 | 1x1 | Logo 1 | `0F/03` | `0x1F` |
| Razer Viper | `0x0078` | 1 | 1x1 | Logo 1 | `0F/03` | `0x3F` |
| Razer Viper 8kHz | `0x0091` | 1 | 1x1 | Logo 1 | `0F/03` | `0x1F` |
| Razer Viper Mini | `0x008A` | 1 | 1x1 | viper_mini_logo_zone ? | `0F/03` | `0x3F` |
| Razer Viper Ultimate (Wired) | `0x007A` | 1 | 1x1 | viper_ultimate_wired_logo_zone ? | `0F/03` | `0x3F` |
| Razer Viper Ultimate (Wireless) | `0x007B` | 1 | 1x1 | viper_ultimate_wireless_logo_zone ? | `0F/03` | `0x3F` |

## Mice with a per-LED matrix in OpenRazer only

| Model | PID | LEDs | Rows x cols |
|---|---|---|---|
| Razer Naga X | `0x0096` | 2 | 1x2 |
| Razer DeathAdder V2 Lite | `0x00A1` | 1 | 1x1 |

## Mice without a per-LED matrix (OpenRazer, not in OpenRGB)

No RGB, or only fixed on-board effects. Recent ones: ProClickReceiver (`0x0077`), ProClickWired (`0x0080`), BasiliskXHyperSpeed (`0x0083`), OrochiV2Receiver (`0x0094`), OrochiV2Bluetooth (`0x0095`), ProClickMiniReceiver (`0x009A`), DeathAdderV2XHyperSpeed (`0x009C`), ViperMiniSEWired (`0x009E`), ViperMiniSEWireless (`0x009F`), ViperV2ProWired (`0x00A5`), ViperV2ProWireless (`0x00A6`), DeathAdderV3 (`0x00B2`), HyperPollingWirelessDongle (`0x00B3`), NagaV2HyperSpeedReceiver (`0x00B4`), DeathAdderV3ProWired (`0x00B6`), DeathAdderV3ProWireless (`0x00B7`), ViperV3HyperSpeed (`0x00B8`), DeathAdderV4ProWired (`0x00BE`), DeathAdderV4ProWireless (`0x00BF`), ViperV3ProWired (`0x00C0`), ViperV3ProWireless (`0x00C1`), DeathAdderV3ProWired_Alternate (`0x00C2`), DeathAdderV3ProWireless_Alternate (`0x00C3`), DeathAdderV3HyperSpeedWired (`0x00C4`), DeathAdderV3HyperSpeedWireless (`0x00C5`), ProClickV2VerticalEditionWired (`0x00C7`), ProClickV2VerticalEditionWireless (`0x00C8`), ProClickV2Wired (`0x00D0`), ProClickV2Wireless (`0x00D1`), BasiliskMobileWired (`0x00D3`), BasiliskMobileReceiver (`0x00D4`), CobraHyperSpeed (`0x00DA`), CobraHyperSpeedWireless (`0x00DB`), ViperV3ProSEWired (`0x00DE`), ViperV3ProSEWireless (`0x00DF`).

Older ones: Orochi2011, Naga, DeathAdder3_5G, NagaEpic, Abyssus1800, DeathAdder3_5GBlack, Naga2012, Imperator, Ouroboros, Taipan, NagaHexRed, DeathAdder2013, DeathAdder1800, Orochi2013, NagaEpicChromaWireless, Naga2014, NagaHex, Abyssus, OrochiWired, DeathAdder2000, DeathAdder3500, AbyssusV2, Abyssus2000, AtherisReceiver.
