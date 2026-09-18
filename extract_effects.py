"""Estrae dai log di Synapse l'ultima definizione di ogni effetto Chroma Studio.
Uso: python extract_effects.py   ->  scrive chroma_effects_dump.json e stampa un riassunto."""
import glob, json, os, re

LOGS = os.path.expandvars(r"%LOCALAPPDATA%\Razer\RazerAppEngine\User Data\Logs")
OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "chroma_effects_dump.json")


def extract(verbose=True):
    """Ritorna {titolo: effetto} e scrive chroma_effects_dump.json."""
    # dal piu' vecchio al piu' recente, cosi' l'ultima occorrenza vince
    files = sorted(glob.glob(os.path.join(LOGS, "chroma-studio*.log")), key=os.path.getmtime)
    txt = "".join(open(f, encoding="utf-8", errors="ignore").read() for f in files)

    dec = json.JSONDecoder()
    effects = {}
    for m in re.finditer(r'"title":"(Chroma Effect[^"]*)"', txt):
        start = m.start()
        for _ in range(6):  # risale all'oggetto JSON che contiene il titolo
            start = txt.rfind("{", 0, start)
            if start < 0:
                break
            try:
                obj, _ = dec.raw_decode(txt[start:])
            except ValueError:
                continue
            if isinstance(obj, dict) and "layers" in obj:
                effects[m.group(1)] = obj
                break

    json.dump(effects, open(OUT, "w", encoding="utf-8"), indent=1)
    for title, e in effects.items():
        if verbose: print(f"\n== {title}")
        for layer in e["layers"]:
            vis = "ON " if layer.get("visible") else "off"
            for reg in layer.get("regions", []):
                n = sum(len(d["leds"]) for d in reg["devices"])
                if verbose: print(f"  [{vis}] {layer['title']:<16} {n:>2} LED  {reg['params']}")
    if verbose: print("\nsalvato:", OUT)
    return effects




PROFILES_OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "synapse_profiles.json")


def extract_profiles(verbose=True):
    """Profili Synapse del mouse con il loro slot onboard, dai log del modulo mouse."""
    files = sorted(glob.glob(os.path.join(LOGS, "products_203_mw*.log")), key=os.path.getmtime)
    txt = "".join(open(f, encoding="utf-8", errors="ignore").read() for f in files)
    dec = json.JSONDecoder()
    last = {}
    for m in re.finditer(r'\{"activeProfile":', txt):
        try:
            obj, _ = dec.raw_decode(txt[m.start():])
        except ValueError:
            continue
        p = obj["activeProfile"]
        if p.get("name") and p.get("obmSlotId"):
            last[p["guid"]] = {"name": p["name"], "slot": p["obmSlotId"][0]}
    # se due profili risultano sullo stesso slot vince l'ultimo visto
    by_slot = {}
    for guid, p in last.items():
        by_slot[p["slot"]] = p
    profiles = sorted(by_slot.values(), key=lambda p: p["slot"])
    json.dump(profiles, open(PROFILES_OUT, "w", encoding="utf-8"), indent=1)
    if verbose:
        for p in profiles:
            print(f"slot {p['slot']}: {p['name']}")
    return profiles


if __name__ == "__main__":
    extract()
    print()
    extract_profiles()
