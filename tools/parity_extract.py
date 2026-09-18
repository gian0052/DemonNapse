"""Confronta l'estrazione dai log di Synapse e l'abbinamento profilo-effetto fatti da Rust
con quelli di Python (il riferimento).

Uso:  python tools/parity_extract.py    (dopo `cargo build --release`)

Entrambe scrivono in una cartella temporanea: i file usati dal demone non vengono toccati.
"""
import json
import os
import subprocess
import sys
import logging
import tempfile

HERE = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, HERE)
logging.basicConfig(level=logging.ERROR)
import demonnapse as d
import extract_effects as ex

EXE = os.path.join(HERE, "target", "release", "demonnapse.exe")
FILES = ("chroma_effects_dump.json", "synapse_profiles.json")


def main():
    failed = 0
    with tempfile.TemporaryDirectory() as py, tempfile.TemporaryDirectory() as rs:
        ex.OUT, ex.PROFILES_OUT = (os.path.join(py, f) for f in FILES)
        ex.extract(verbose=False)
        ex.extract_profiles(verbose=False)
        out = subprocess.run([EXE, "extract", "--out", rs], capture_output=True, text=True, cwd=HERE)
        if out.returncode:
            raise RuntimeError(out.stderr.strip())
        for f in FILES:
            a = json.load(open(os.path.join(py, f), encoding="utf-8"))
            b = json.load(open(os.path.join(rs, f), encoding="utf-8"))
            ok = a == b
            failed += not ok
            print("%-26s %s | %d voci" % (f, "OK" if ok else "DIVERSO", len(b)))
            if not ok and isinstance(a, dict):
                for k in sorted(set(a) | set(b)):
                    if a.get(k) != b.get(k):
                        print("   diverso:", k)
    # abbinamento profilo -> effetto, sui file che il demone usa davvero
    cfg, _ = d.load()
    out = subprocess.run([EXE, "profiles"], capture_output=True, text=True, cwd=HERE)
    if out.returncode:
        raise RuntimeError(out.stderr.strip())
    ok = json.loads(out.stdout) == cfg["profiles"]
    failed += not ok
    print("%-26s %s | %s" % ("profilo -> effetto", "OK" if ok else "DIVERSO", out.stdout.strip()))
    print("\n%s" % ("tutto coincide" if not failed else "%d confronti diversi" % failed))
    return failed


if __name__ == "__main__":
    sys.exit(main())
