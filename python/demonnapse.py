"""DemonNapse - lightweight replacement for Razer Synapse RGB on the Basilisk V3 35K.

Legge gli effetti da chroma_effects_dump.json (estratti da Chroma Studio con
extract_effects.py), capisce il profilo onboard attivo dagli eventi HID del
mouse e renderizza l'effetto associato. Synapse deve essere CHIUSO.

Uso:  python demonnapse.py [-v]      (pythonw per girare senza finestra)
"""
import ctypes
import json
import logging
import os
import random
import re
import socket
import subprocess
import sys
import time

import hid
import psutil

import chroma_layers as cl
import razerhid as rz
from chroma_layers import BLACK, gradient, lerp, rgb

HERE = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))  # la radice del repo: config.json e i file estratti
log = logging.getLogger("demonnapse")
logging.getLogger("comtypes").setLevel(logging.WARNING)

# posizione nella griglia di Chroma Studio -> indice LED fisico (dal config: dipende dal mouse)
GRID = {}
PAIR_WINDOW = 0.25  # s: "05 02" seguito da "05 39" entro questo tempo = cambio profilo
VK_BUTTONS = (0x01, 0x02, 0x04, 0x05, 0x06)  # sx, dx, centrale, laterali


# ---------------------------------------------------------------- audio
class PeakMeter:
    """Picco dell'uscita audio di sistema (IAudioMeterInformation): niente cattura, costo ~zero."""

    def __init__(self):
        self.meter = None
        self.acquired = 0.0

    def _acquire(self):
        from comtypes import CLSCTX_ALL
        from pycaw.pycaw import AudioUtilities, IAudioMeterInformation
        dev = AudioUtilities.GetSpeakers()
        dev = getattr(dev, "_dev", dev)  # pycaw recente incapsula l'IMMDevice
        # QueryInterface e non cast(): cast() duplica il puntatore senza AddRef, e alla
        # riacquisizione la doppia Release manda in access violation tutto il processo
        self.meter = dev.Activate(IAudioMeterInformation._iid_, CLSCTX_ALL, None) \
                        .QueryInterface(IAudioMeterInformation)
        self.acquired = time.monotonic()

    def peak(self):
        try:
            # riaggancia ogni 10s: segue il cambio di dispositivo predefinito
            if self.meter is None or time.monotonic() - self.acquired > 10:
                self._acquire()
            return float(self.meter.GetPeakValue())
        except Exception as e:  # dispositivo sparito, COM, ecc.
            log.debug("audio: %s", e)
            self.meter = None
            return 0.0


# ---------------------------------------------------------------- effetti
class Effect:
    def __init__(self, spec, cfg):
        self.title = spec["title"]
        self.cfg = cfg
        self.base = []       # fondo, dal basso verso l'alto: layer di chroma_layers
        self.audio = []      # (leds, stops, boost, decay)
        self.reactive = []   # (leds, color|None, duration_s)
        self.flashes = []    # (leds, color, t0, duration_s)
        self.level = 0.0
        self.last_audio = -1e9  # ultimo istante con audio sopra la soglia
        # in Chroma Studio il primo layer della lista sta sopra: applico dal fondo
        for layer in reversed(spec["layers"]):
            if not layer.get("visible", True):
                continue
            for reg in layer["regions"]:
                leds = [GRID[(l["row"], l["col"])] for d in reg["devices"] for l in d["leds"]]
                p = reg["params"]
                if layer["name"] in cl.LAYERS:
                    self.base.append(cl.LAYERS[layer["name"]](leds, p, layer.get("params") or {}))
                elif layer["name"] == "audio":
                    stops = [(s["Stop"], rgb(s["Color"])) for s in p["colorStops"]]
                    self.audio.append((leds, stops, p.get("boost", 1), p.get("decay", 0.5)))
                elif layer["name"] == "reactive":
                    color = None if p.get("randomColor") else rgb(p.get("color", 0x00FF00))
                    self.reactive.append((leds, color, p.get("duration", 1500) / 1000))
                else:
                    log.warning("%s: layer '%s' non supportato, ignorato", self.title, layer["name"])
                    break   # un avviso per layer, non uno per regione

    @property
    def needs_clicks(self):
        return bool(self.reactive) or any(l.on_press for l in self.base)

    def on_click(self, now):
        for l in self.base:
            if l.on_press:
                l.press(now)
        for leds, color, dur in self.reactive:
            c = color or rgb(random.choice((0xFF0000, 0x00FF00, 0x0000FF, 0xFFFF00, 0xFF00FF, 0x00FFFF)))
            self.flashes.append((leds, c, now, dur))

    def idle(self, now):
        """True se l'audio tace da abbastanza: tocca a statico e reactive."""
        return not self.audio or now - self.last_audio > self.cfg.get("audio_idle_delay", 3.0)

    def render(self, now, dt, meter):
        floor = self.cfg.get("audio_silence_floor", 0.02)
        for leds, stops, boost, decay in self.audio:
            peak = min(1.0, meter.peak() * boost * self.cfg["audio_gain"])
            self.level = max(peak, self.level - dt / max(decay, 0.05))
            if self.level > floor:
                self.last_audio = now
        if not self.idle(now):
            # l'audio comanda da solo: base spenta, niente lampi
            self.flashes.clear()
            frame = [BLACK] * rz.NUM_LEDS
            for leds, stops, boost, decay in self.audio:
                if self.level > floor:
                    c = gradient(stops, self.level)
                    c = lerp(BLACK, c, min(1.0, (self.level - floor) / 0.06))  # accensione morbida
                    for i in leds:
                        frame[i] = c
            return frame
        frame = [BLACK] * rz.NUM_LEDS
        for l in self.base:
            l.paint(now, frame)
        self.flashes = [f for f in self.flashes if now - f[2] < f[3]]
        for leds, color, t0, dur in self.flashes:
            k = 1.0 - (now - t0) / dur
            for i in leds:
                frame[i] = lerp(frame[i], color, k)
        return frame


# ---------------------------------------------------------------- profili
class ProfileTracker:
    """Legge lo slot attivo dal mouse; gli slot non configurati (es. la Hybrid
    Storage) vengono saltati passando al successivo configurato."""

    def __init__(self, ctl, profiles):
        self.ctl = ctl
        self.slots = [p["slot"] for p in profiles]
        self.idx = 0
        self.last_poll = 0.0
        self.refresh(time.monotonic())

    def refresh(self, now):
        """Ritorna True se il profilo attivo e' cambiato."""
        self.last_poll = now
        slot = rz.get_active_profile(self.ctl)
        if slot is None:
            return False
        if slot not in self.slots:
            nxt = next((s for s in self.slots if s > slot), self.slots[0])
            log.info("slot %d non configurato -> salto allo slot %d", slot, nxt)
            rz.set_active_profile(self.ctl, nxt)
            slot = nxt
        new = self.slots.index(slot)
        changed = new != self.idx
        self.idx = new
        return changed

    def feed(self, report, now):
        # qualunque evento "05 xx" (DPI, rotella, cambio profilo) -> rileggo lo slot
        if len(report) >= 2 and report[0] == 0x05:
            return self.refresh(now)
        return False


def open_event_handles():
    out = []
    for d in hid.enumerate(rz.VID, rz.PID):
        if d["usage_page"] == 0x0001 and d["usage"] == 0x0000:
            h = hid.device()
            try:
                h.open_path(d["path"])
                h.set_nonblocking(True)
                out.append(h)
            except OSError:
                pass
    return out


# ---------------------------------------------------------------- Synapse
SYNAPSE_EXES = {"razerappengine.exe"}          # Synapse 4 e Chroma App
RAZER_EXES = {"razerappengine.exe", "razeraxon.exe"} # da uccidere se restano appesi
RAZER_SERVICES = ["RzActionSvc", "RazerExperienceService", "Razer Game Manager Service 3", "RGS",
                  "Razer Chroma SDK Server", "Razer Chroma SDK Service", "Razer Chroma SDK Diagnostic Service",
                  "Razer Chroma Stream Server", "Razer Elevation Service"]


def is_admin():
    return bool(ctypes.windll.shell32.IsUserAnAdmin())


def synapse_running():
    for p in psutil.process_iter(["name"]):
        if (p.info["name"] or "").lower() in SYNAPSE_EXES:
            return True
    return False


def kill_razer_apps():
    for p in psutil.process_iter(["name"]):
        if (p.info["name"] or "").lower() in RAZER_EXES:
            try:
                p.kill()
                log.info("ucciso %s (pid %d)", p.info["name"], p.pid)
            except psutil.Error:
                pass


def razer_services(action):
    """action: 'start' | 'stop'. Richiede privilegi di amministratore."""
    if not is_admin():
        log.warning("non sono amministratore: servizi Razer lasciati come sono (%s)", action)
        return
    flags = subprocess.CREATE_NO_WINDOW
    for svc in RAZER_SERVICES:
        subprocess.run(["sc", action, svc], capture_output=True, creationflags=flags)
    log.info("servizi Razer: %s", action)


# ---------------------------------------------------------------- main
def _tokens(name):
    name = re.sub(r"^chroma effect\s*", "", name.lower())
    return {t for t in re.split(r"[^a-z0-9]+", name) if t}


def match_effect(profile_name, titles, overrides):
    """Effetto Chroma per un profilo Synapse: override esplicito, poi parole in comune nei nomi."""
    if profile_name in overrides:
        return overrides[profile_name]
    want = _tokens(profile_name)
    best, score = None, 0
    for t in titles:
        common = len(want & _tokens(t))
        if common > score:
            best, score = t, common
    return best


def load():
    cfg = json.load(open(os.path.join(HERE, "config.json"), encoding="utf-8"))
    dev = cfg["device"]
    if not dev.get("chroma_grid"):  # mappa dei LED letta dai log di Synapse dal demone Rust (synapse_device.json)
        dev.update(json.load(open(os.path.join(HERE, "synapse_device.json"), encoding="utf-8")))
    rz.configure(int(dev["pid"], 16), dev["leds"])
    GRID.clear()
    GRID.update({(int(k.split(",")[0]), int(k.split(",")[1])): v for k, v in dev["chroma_grid"].items()})
    cl.set_positions(GRID)
    dump = json.load(open(os.path.join(HERE, "chroma_effects_dump.json"), encoding="utf-8"))
    synapse = json.load(open(os.path.join(HERE, "synapse_profiles.json"), encoding="utf-8"))
    profiles, effects = [], []
    for p in synapse:
        if p["slot"] in cfg["skip_slots"]:
            continue
        title = match_effect(p["name"], dump.keys(), cfg["effect_overrides"])
        if title:
            fx = Effect(dump[title], cfg)
        else:
            log.warning("profilo '%s' (slot %d): nessun effetto Chroma abbinato, RGB spento", p["name"], p["slot"])
            fx = Effect({"title": "(spento)", "layers": []}, cfg)
        log.info("slot %d  %-18s -> %s", p["slot"], p["name"], fx.title)
        profiles.append({"slot": p["slot"], "name": p["name"], "effect": fx.title})
        effects.append(fx)
    if not profiles:
        raise RuntimeError("nessun profilo Synapse trovato: apri Synapse una volta e richiudilo")
    cfg["profiles"] = profiles
    return cfg, effects


def wait_for_synapse_to_close():
    """Synapse e' aperto: gli lascio il mouse e i servizi, e aspetto."""
    log.info("Synapse aperto: mi metto in pausa")
    razer_services("start")
    while synapse_running():
        time.sleep(1.0)
    log.info("Synapse chiuso: rileggo gli effetti e riprendo")
    try:
        import extract_effects
        extract_effects.extract(verbose=False)
        extract_effects.extract_profiles(verbose=False)
    except Exception as e:
        log.error("estrazione effetti fallita, tengo i vecchi: %s", e)
    time.sleep(2.0)            # lascio finire i processi figli di Synapse
    kill_razer_apps()
    razer_services("stop")


def main():
    verbose = "-v" in sys.argv
    logging.basicConfig(
        level=logging.DEBUG if verbose else logging.INFO,
        format="%(asctime)s %(message)s", datefmt="%H:%M:%S",
        handlers=[logging.StreamHandler()] if sys.stdout else
                 [logging.FileHandler(os.path.join(HERE, "demonnapse.log"), encoding="utf-8")])

    # una sola istanza: due demoni sullo stesso mouse si pestano i piedi
    ctypes.windll.kernel32.CreateMutexW(None, True, r"Local\demonnapse")
    if ctypes.windll.kernel32.GetLastError() == 183:  # ERROR_ALREADY_EXISTS
        log.error("DemonNapse e' gia' in esecuzione, esco")
        return

    log.info("avvio: pulizia processi e servizi Razer")
    kill_razer_apps()
    razer_services("stop")

    meter = PeakMeter()
    user32 = ctypes.windll.user32
    wall = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)   # lo sfondo (wallpaper.py) ascolta qui
    WALL_ADDR = ("127.0.0.1", 47311)

    while True:  # un giro per ogni "sessione" tra un'apertura di Synapse e l'altra
        cfg, effects = load()
        ctl, _, _ = rz.open_control()
        events = open_event_handles()
        tracker = ProfileTracker(ctl, cfg["profiles"])
        log.info("attivo: slot %d -> profilo '%s'", tracker.slots[tracker.idx], cfg["profiles"][tracker.idx]["name"])

        pressed = [False] * len(VK_BUTTONS)
        tick = 1.0 / cfg["fps"]
        last = time.monotonic()
        last_frame, last_sent, last_syn = None, 0.0, 0.0

        while True:
            now = time.monotonic()
            dt, last = now - last, now
            fx = effects[tracker.idx]

            if now - last_syn > 1.5:
                last_syn = now
                if synapse_running():
                    break

            changed = False
            for h in events:
                while True:
                    data = h.read(64)
                    if not data:
                        break
                    log.debug("evento %s", " ".join("%02X" % b for b in data[:6]))
                    changed |= tracker.feed(data, now)
            if now - tracker.last_poll > 30.0:         # rete di sicurezza se un evento si perde
                changed |= tracker.refresh(now)
            if changed:
                fx = effects[tracker.idx]
                fx.flashes.clear()
                last_frame = None
                log.info("profilo -> '%s' (%s)", cfg["profiles"][tracker.idx]["name"], fx.title)

            if fx.needs_clicks and fx.idle(now):
                for n, vk in enumerate(VK_BUTTONS):
                    down = bool(user32.GetAsyncKeyState(vk) & 0x8000)
                    if down and not pressed[n]:
                        fx.on_click(now)
                    pressed[n] = down

            frame = fx.render(now, dt, meter)
            # reinvio solo se cambia; keepalive ogni secondo (il firmware puo' ridisegnare il suo effetto)
            if frame != last_frame or now - last_sent > 1.0:
                rz.custom_frame(ctl, frame)
                try:
                    wall.sendto(bytes(c for led in frame for c in led), WALL_ADDR)
                except OSError:
                    pass
                last_frame, last_sent = frame, now

            time.sleep(max(0.0, tick - (time.monotonic() - now)))

        # Synapse e' comparso: mollo il mouse
        ctl.close()
        for h in events:
            h.close()
        wait_for_synapse_to_close()


if __name__ == "__main__":
    # i crash nativi (access violation in una DLL) non passano da except: li scrivo su file
    import faulthandler
    faulthandler.enable(open(os.path.join(HERE, "demonnapse.crash.log"), "a"))
    try:
        main()
    except KeyboardInterrupt:
        pass
    except Exception:
        log.exception("errore fatale")   # sotto pythonw non c'e' stderr: il traceback va nel log
        raise
