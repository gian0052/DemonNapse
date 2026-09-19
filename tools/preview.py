"""Mostra sul mouse un effetto di Chroma Studio senza assegnarlo a un profilo.

Uso:  python tools/preview.py "Chroma Effect TEST" [secondi] [--layer wave] [--all]
      python tools/preview.py            (elenca gli effetti disponibili)

--layer NOME  mostra solo quel layer dell'effetto (prima regione)
--all         lo stende su tutti i LED del mouse, per vedere bene gli effetti che si muovono

Ferma il demone per la durata dell'anteprima e lo fa ripartire alla fine. Synapse deve
essere chiuso. Serve per confrontare a occhio il rendering con quello di Synapse.
"""
import ctypes
import json
import os
import subprocess
import sys
import time

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "python"))
import demonnapse as d
import razerhid as rz

TASK = "DemonNapse"


def schtasks(action):
    return subprocess.run(["schtasks", "/" + action, "/TN", TASK], capture_output=True,
                          creationflags=subprocess.CREATE_NO_WINDOW).returncode == 0


def main():
    cfg, _ = d.load()
    dump = json.load(open(os.path.join(d.HERE, "chroma_effects_dump.json"), encoding="utf-8"))
    args = [a for a in sys.argv[1:] if not a.startswith("--")]
    only = sys.argv[sys.argv.index("--layer") + 1] if "--layer" in sys.argv else None
    if only in args:
        args.remove(only)
    if not args or args[0] not in dump:
        print("effetti disponibili:")
        for title, e in dump.items():
            print("  %-28s %s" % (title, ", ".join(l["name"] for l in e["layers"])))
        return
    seconds = float(args[1]) if len(args) > 1 else 30.0
    if d.synapse_running():
        print("Synapse e' aperto: chiudilo prima dell'anteprima")
        return
    spec = dump[args[0]]
    if only:
        layers = [dict(l, regions=l["regions"][:1]) for l in spec["layers"] if l["name"] == only]
        if not layers:
            print("'%s' non ha un layer '%s'" % (args[0], only))
            return
        spec = dict(spec, layers=layers[:1])
    if "--all" in sys.argv:
        everywhere = [{"leds": [{"row": r, "col": c} for r, c in d.GRID]}]
        spec = dict(spec, layers=[dict(l, regions=[dict(r, devices=everywhere) for r in l["regions"]])
                                  for l in spec["layers"]])
    fx = d.Effect(spec, cfg)
    for l in spec["layers"]:
        for r in l["regions"]:
            print("  %s: %s" % (l["name"], {k: v for k, v in r["params"].items() if k != "colorStops"}))

    was_running = schtasks("End")
    time.sleep(1.0)   # il demone molla il mouse
    ctl, _, _ = rz.open_control()
    meter, user32 = d.PeakMeter(), ctypes.windll.user32
    pressed = [False] * len(d.VK_BUTTONS)
    tick, last = 1.0 / cfg["fps"], time.monotonic()
    end = last + seconds
    print("anteprima di '%s' per %.0f s (Ctrl+C per uscire)" % (fx.title, seconds))
    try:
        while time.monotonic() < end:
            now = time.monotonic()
            dt, last = now - last, now
            if fx.needs_clicks and fx.idle(now):
                for n, vk in enumerate(d.VK_BUTTONS):
                    down = bool(user32.GetAsyncKeyState(vk) & 0x8000)
                    if down and not pressed[n]:
                        fx.on_click(now)
                    pressed[n] = down
            rz.custom_frame(ctl, fx.render(now, dt, meter))
            time.sleep(max(0.0, tick - (time.monotonic() - now)))
    except KeyboardInterrupt:
        pass
    finally:
        ctl.close()
        if was_running:
            print("riavvio il demone:", "ok" if schtasks("Run") else "FALLITO, lancia install.cmd")


if __name__ == "__main__":
    main()
