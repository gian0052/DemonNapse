"""Razer HID sniffer: shows what the mouse sends to the PC (e.g. the profile button).
Usage: CLOSE Synapse completely (from the tray too), then:  python tools/sniff.py
Press the profile button under the mouse and watch what appears. Ctrl+C to quit."""
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
        print(f"[opened]  {tag}  {d['product_string']}")
    except OSError:
        # standard mouse/keyboard: Windows holds them exclusively, that is normal
        print(f"[skipped] {tag}")

print("\nListening. Press the profile button...\n")
try:
    while True:
        for tag, h in list(devs):
            try:
                data = h.read(256)
            except OSError:
                # interface held exclusively by Windows: I drop it and carry on
                print(f"[dropped] {tag}")
                devs.remove((tag, h))
                continue
            if data:
                ts = time.strftime("%H:%M:%S")
                print(f"{ts}  {tag}  ->  {' '.join(f'{b:02X}' for b in data)}")
        time.sleep(0.005)
except KeyboardInterrupt:
    pass
