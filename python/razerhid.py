"""Protocollo HID Razer minimale (formato report da OpenRazer)."""
import time
import hid

VID, PID = 0x1532, 0x00CB      # Razer Basilisk V3 35K (cablato); sovrascritti da configure()
NUM_LEDS = 11


def configure(pid, num_leds):
    """Imposta il mouse da usare (PID USB) e il numero di LED. Chiamare prima di open_control()."""
    global PID, NUM_LEDS
    PID, NUM_LEDS = pid, num_leds
TRANSACTION_ID = 0x1F


def build(cmd_class, cmd_id, data_size, args=b""):
    r = bytearray(90)
    r[1] = TRANSACTION_ID
    r[5] = data_size
    r[6] = cmd_class
    r[7] = cmd_id
    r[8:8 + len(args)] = args
    crc = 0
    for b in r[2:88]:
        crc ^= b
    r[88] = crc
    return bytes(r)


def transact(h, report):
    h.send_feature_report(b"\x00" + report)
    time.sleep(0.05)
    return bytes(h.get_feature_report(0, 91))[1:]


def open_control(retries=5):
    """Trova la collection che accetta i feature report da 90 byte."""
    for attempt in range(retries):
        try:
            return _open_control()
        except RuntimeError:
            if attempt == retries - 1:
                raise
            time.sleep(1)  # un'altra istanza sta morendo o il mouse e' occupato


def _open_control():
    probe = build(0x00, 0x81, 0x02)  # get firmware version (sola lettura)
    for d in hid.enumerate(VID, PID):
        h = hid.device()
        try:
            h.open_path(d["path"])
            resp = transact(h, probe)
            if len(resp) >= 10 and resp[0] == 0x02 and resp[6] == 0x00 and resp[7] == 0x81:
                return h, d, resp
        except OSError:
            pass
        h.close()
    raise RuntimeError("nessuna interfaccia di controllo trovata")


def static_color(h, r, g, b, store=0x00):
    """Effetto statico su tutti i LED. store=0x00 non persiste, 0x01 salva."""
    args = bytes([store, 0x00, 0x01, 0x00, 0x00, 0x01, r, g, b])
    return transact(h, build(0x0F, 0x02, 0x09, args))


def custom_frame(h, colors):
    """Colore per singolo LED (lista di tuple RGB, una per LED)."""
    n = len(colors)
    args = bytes([0x00, 0x00, 0x00, 0x00, n - 1]) + bytes(c for rgb in colors for c in rgb)
    h.send_feature_report(b"\x00" + build(0x0F, 0x03, len(args), args))


# Mappa LED del Basilisk V3 35K (verificata a occhio, un LED alla volta):
#   0 logo, 1 rotella, 2-10 striscia (SX alto -> SX basso -> retro -> DX basso -> DX alto)


# Profili onboard (classe 0x05). Verificati sul Basilisk V3 35K:
#   get 0x80 = numero profili, 0x81 = [capienza, slot...], 0x84 = slot attivo
#   set 0x04 = cambia slot attivo (il mouse emette gli stessi eventi del pulsante)
def get_active_profile(h):
    resp = transact(h, build(0x05, 0x84, 0x01))
    return resp[8] if resp[0] == 0x02 else None


def set_active_profile(h, slot):
    return transact(h, build(0x05, 0x04, 0x01, bytes([slot])))[0] == 0x02
