"""Measures the running daemon: CPU, RAM and frames sent to the wallpaper over UDP.

Usage:  python tools/bench.py [seconds]      (the dynamic wallpaper must be off: the port is needed here)

Finds the demonnapse.exe process by itself.
"""
import collections
import socket
import subprocess
import sys
import time

import psutil

SECONDS = float(sys.argv[1]) if len(sys.argv) > 1 else 20


def find():
    for p in psutil.process_iter(["name", "cmdline"]):
        name = (p.info["name"] or "").lower()
        try:
            cmd = " ".join(p.info["cmdline"] or [])
        except psutil.Error:
            cmd = ""
        if name == "demonnapse.exe" and len(p.info["cmdline"] or []) <= 2 and "bench" not in cmd:
            return p
    return None


def private_ws(p):
    """Private working set in MB (what Task Manager shows)."""
    try:
        return p.memory_full_info().uss / 2**20
    except psutil.Error:
        pass
    # elevated process (the task runs with high privileges): I ask the Windows counters for it
    out = subprocess.run(["powershell", "-NoProfile", "-Command",
                          "(Get-CimInstance Win32_PerfFormattedData_PerfProc_Process -Filter 'IDProcess=%d').WorkingSetPrivate" % p.pid],
                         capture_output=True, text=True).stdout.strip()
    return int(out) / 2**20 if out.isdigit() else float("nan")


def main():
    p = find()
    if not p:
        sys.exit("no daemon running")
    print("process: %s (pid %d)" % (p.name(), p.pid))
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("127.0.0.1", 47311))
    sock.settimeout(0.2)
    frames = collections.Counter()
    cpu0, t0, n = sum(p.cpu_times()[:2]), time.monotonic(), 0
    while time.monotonic() - t0 < SECONDS:
        try:
            frames[sock.recv(256)] += 1
            n += 1
        except socket.timeout:
            pass
    dt = time.monotonic() - t0
    cpu = sum(p.cpu_times()[:2]) - cpu0
    print("CPU: %.2f%% of one core (%.2fs over %.0fs)" % (100 * cpu / dt, cpu, dt))
    print("RAM: private working set %.1f MB, working set %.1f MB" % (private_ws(p), p.memory_info().rss / 2**20))
    print("UDP frames: %d (%.1f/s), %d distinct" % (n, n / dt, len(frames)))
    for f, c in frames.most_common(3):
        print("  x%-5d %s" % (c, " ".join(f[i:i + 3].hex().upper() for i in range(0, len(f), 3))))


if __name__ == "__main__":
    main()
