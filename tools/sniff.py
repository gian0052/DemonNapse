"""Sniffer HID Razer: mostra cosa manda il mouse al PC (es. pulsante profilo).
Uso: CHIUDI Synapse del tutto (anche dal tray), poi:  python sniff.py
Premi il pulsante profilo sotto il mouse e guarda cosa compare. Ctrl+C per uscire."""
import time
import hid

RAZER_VID = 0x1532

devs = []
for d in hid.enumerate(RAZER_VID):
    tag = f"PID={d['product_id']:04X} MI={d['interface_number']} usage={d['usage_page']:04X}:{d['usage']:04X}"
    try:
        h = hid.device()
        h.open_path(d["path"])
        h.set_nonblocking(True)
        devs.append((tag, h))
        print(f"[aperto]  {tag}  {d['product_string']}")
    except OSError:
        # mouse/tastiera standard: Windows li tiene in esclusiva, normale
        print(f"[saltato] {tag}")

print("\nIn ascolto. Premi il pulsante profilo...\n")
try:
    while True:
        for tag, h in list(devs):
            try:
                data = h.read(256)
            except OSError:
                # interfaccia in esclusiva a Windows: la scarto e continuo
                print(f"[scartato] {tag}")
                devs.remove((tag, h))
                continue
            if data:
                ts = time.strftime("%H:%M:%S")
                print(f"{ts}  {tag}  ->  {' '.join(f'{b:02X}' for b in data)}")
        time.sleep(0.005)
except KeyboardInterrupt:
    pass
