"""Rendering dei layer di Chroma Studio, con i parametri come li scrive Synapse nei log.

Ogni layer ha .leds, .on_press e paint(now, frame): dipinge sui propri LED sopra quello
che c'e' gia' (alpha < 1 = lascia vedere i layer sotto). Audio Meter e Reactive restano in
demonnapse.py perche' dipendono dal meter e dai click.

La forma di ogni effetto segue il modo in cui Chroma Studio lo descrive al suo motore
(gradiente, durata, larghezza, velocita', densita'). Le unita' di "speed" invece non sono
documentate: le costanti qui sotto sono stime.
"""
import math
import random

BLACK = (0, 0, 0)
DEV_W, DEV_H = 200.0, 320.0      # tela del dispositivo in Chroma Studio (px)
POS = {}                         # indice LED fisico -> (x, y) sulla tela

WAVE_HZ = 1 / 35                 # cicli al secondo per unita' di "speed" (wave e tidal); 1/50 sembrava lento
WHEEL_DEG_S = 2.0                # gradi al secondo per unita' di "speed"; 1.0 sembrava lento
RIPPLE_PX_S = 30.0               # px al secondo per unita' di "speed"
RIPPLE_EVERY = (0.8, 2.5)        # s tra due onde quando playbackStart = "random"
STAR_MAX_DENSITY = 10            # "density" 10 = tutti i LED della regione accesi insieme
STAR_REGEN = 3.0                 # nuove stelle al secondo per LED spento, a density massima


def rgb(c):
    return ((c >> 16) & 255, (c >> 8) & 255, c & 255)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(3))


def gradient(stops, pos):
    """stops: [(0..100, rgb)], pos: 0..1"""
    p = pos * 100
    for (s0, c0), (s1, c1) in zip(stops, stops[1:]):
        if p <= s1:
            return lerp(c0, c1, (p - s0) / (s1 - s0) if s1 > s0 else 0)
    return stops[-1][1]


def set_positions(grid):
    """grid: {(riga, colonna) di Chroma Studio: indice LED}. La griglia copre tutta la tela."""
    rows = max(r for r, _ in grid) or 1
    cols = max(c for _, c in grid) or 1
    POS.clear()
    POS.update({i: (c / cols * DEV_W, r / rows * DEV_H) for (r, c), i in grid.items()})


def _stops(p):
    return [(s["Stop"], rgb(s["Color"])) for s in p["colorStops"]]


def _random_color():
    return gradient([(0, (255, 0, 0)), (33, (0, 255, 0)), (66, (0, 0, 255)), (100, (255, 0, 0))], random.random())


def _along(i, angle):
    """Posizione 0..1 del LED lungo la direzione `angle` attraverso la tela. Gradi come su una
    bussola: 0 = verso l'alto, 90 = verso destra (il wave di default di Chroma Studio), 180 = giu'."""
    a = math.radians(angle)
    dx, dy = math.sin(a), -math.cos(a)
    x, y = POS[i]
    span = abs(DEV_W * dx) + abs(DEV_H * dy)
    return ((x - DEV_W / 2) * dx + (y - DEV_H / 2) * dy) / span + 0.5


class Layer:
    def __init__(self, leds, p, lp):
        self.leds = leds
        self.on_press = p.get("playbackStart") == "onPress"   # parte al primo click
        self.running = not self.on_press

    def press(self, now):
        self.running = True

    def at(self, now, i):
        """(rgb, alpha) per il LED i, oppure None = trasparente."""
        raise NotImplementedError

    def paint(self, now, frame):
        if not self.running:
            return
        for i in self.leds:
            out = self.at(now, i)
            if out:
                c, a = out
                frame[i] = c if a >= 1 else lerp(frame[i], c, a)


class Static(Layer):
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        self.out = (rgb(p["color"]) if "color" in p else BLACK, 1)

    def at(self, now, i):
        return self.out


class Spectrum(Layer):
    """Tutta la regione dello stesso colore, un giro del gradiente ogni `duration` ms."""
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        self.stops, self.period = _stops(p), max(p.get("duration", 37740), 100) / 1000

    def at(self, now, i):
        return gradient(self.stops, (now % self.period) / self.period), 1


class Wave(Layer):
    """Il gradiente scorre attraverso la tela lungo `angle`; `split` = parte dal centro.
    Se `counterclockwise` e' un booleano nei parametri della regione (effetti rapidi del
    mouse) l'onda gira in tondo invece di attraversare la tela."""
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        self.stops, self.angle, self.split = self.make_stops(p), p.get("angle", 90), p.get("split", False)
        self.length = max(p.get("width", 100), 1) / 100          # lunghezze d'onda per tela
        self.hz = p.get("speed", 25) * WAVE_HZ
        self.spin = (-1 if p["counterclockwise"] else 1) if isinstance(p.get("counterclockwise"), bool) else 0
        self.back = bool(lp.get("counterclockwise")) and not self.spin   # verso di marcia invertito

    def make_stops(self, p):
        return _stops(p)

    def at(self, now, i):
        if self.spin:
            x, y = POS[i]
            pos = math.atan2(y - DEV_H / 2, x - DEV_W / 2) / math.tau * self.spin
        else:
            pos = _along(i, self.angle)
            if self.split:
                pos = abs(pos - 0.5) * 2
            if self.back:
                pos = -pos
        return gradient(self.stops, (pos / self.length - now * self.hz) % 1), 1


class Tidal(Wave):
    """Un'onda con un gradiente fisso: buio, un'ondata di color, buio, un'ondata di color2."""
    def make_stops(self, p):
        if isinstance(p.get("colorStops"), list):
            return _stops(p)
        if p.get("randomColor"):
            c1, c2 = _random_color(), _random_color()
            return [(0, BLACK), (15, BLACK), (30, c1), (40, BLACK), (65, BLACK), (70, c2), (90, BLACK), (100, BLACK)]
        c1, c2 = rgb(p.get("color", 0x00FF00)), rgb(p.get("color2", 0x0000FF))
        return [(0, BLACK), (10, BLACK), (25, c1), (40, BLACK), (60, BLACK), (75, c2), (90, BLACK), (100, BLACK)]


class Wheel(Layer):
    """Il gradiente ruota intorno al punto centrale del layer."""
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        self.stops = _stops(p)
        self.cx, self.cy = lp.get("centerPointX", DEV_W / 2), lp.get("centerPointY", DEV_H / 2)
        self.turns = p.get("speed", 60) * WHEEL_DEG_S / 360 * (-1 if lp.get("counterclockwise") else 1)

    def at(self, now, i):
        x, y = POS[i]
        return gradient(self.stops, (math.atan2(y - self.cy, x - self.cx) / math.tau - now * self.turns) % 1), 1


class Ripple(Layer):
    """Anelli che si allargano da un punto: a caso, oppure a ogni click se onPress."""
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        self.stops = _stops(p)
        self.speed = max(p.get("speed", 10), 1) * RIPPLE_PX_S
        self.thick = max(p.get("width", 300), 30) / 3
        self.rings = []          # (t0, x, y)
        self.next = 0.0
        self.running = True      # onPress qui vuol dire "un anello per click", non "parte al click"

    def press(self, now):
        self.rings.append((now, DEV_W / 2, DEV_H / 2))

    def paint(self, now, frame):
        if not self.on_press and now >= self.next:
            self.rings.append((now,) + random.choice(list(POS.values())))
            self.next = now + random.uniform(*RIPPLE_EVERY)
        reach = math.hypot(DEV_W, DEV_H) + self.thick
        self.rings = [r for r in self.rings if (now - r[0]) * self.speed < reach]
        super().paint(now, frame)

    def at(self, now, i):
        x, y = POS[i]
        best = None
        for t0, rx, ry in self.rings:
            k = ((now - t0) * self.speed - math.hypot(x - rx, y - ry)) / self.thick   # 0 = fronte, 1 = coda
            if 0 <= k <= 1 and (best is None or k < best):
                best = k
        return None if best is None else (gradient(self.stops, best), 1 - best)


class Starlight(Layer):
    """Stelle che nascono a caso: il gradiente e' la vita di una stella (`duration` ms),
    `density` decide quante possono essere accese insieme (10 = tutta la regione)."""
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        self.stops, self.rnd = _stops(p), p.get("randomColor", False)
        self.life = max(p.get("duration", 1700), 100) / 1000
        dens = min(max(p.get("density", 1), 0), STAR_MAX_DENSITY) / STAR_MAX_DENSITY
        self.max = max(1, round(len(leds) * dens)) if dens else 0
        self.rate = dens * STAR_REGEN
        self.stars = {}          # LED -> (t0, colore fisso | None = segue il gradiente)
        self.last = None

    def paint(self, now, frame):
        dt = 0.0 if self.last is None else min(now - self.last, 0.5)
        self.last = now
        for i in self.leds:
            s = self.stars.get(i)
            if s and now - s[0] >= self.life:
                del self.stars[i]
                s = None
            if not s and len(self.stars) < self.max and random.random() < self.rate * dt:
                self.stars[i] = (now, _random_color() if self.rnd else None)
        super().paint(now, frame)

    def at(self, now, i):
        s = self.stars.get(i)
        if not s:
            return None
        k = (now - s[0]) / self.life
        if s[1]:                                   # colore a caso: si accende e si spegne
            return s[1], 1 - abs(2 * k - 1)
        return gradient(self.stops, k), min(1.0, k / 0.15, (1 - k) / 0.15)


class Breathing(Layer):
    """Un ciclo ogni `duration` ms: buio, respiro di color, buio, respiro di color2, buio."""
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        self.p, self.rnd = p, p.get("randomColor", False)
        self.period = max(p.get("duration", 7000), 200) / 1000
        self.cycle, self.stops = -1, None

    def at(self, now, i):
        n = int(now / self.period)
        if n != self.cycle:
            self.cycle = n
            if isinstance(self.p.get("colorStops"), list) and not self.rnd:
                self.stops = _stops(self.p)
            else:
                c1 = _random_color() if self.rnd else rgb(self.p.get("color", 0x00FF00))
                c2 = _random_color() if self.rnd else (rgb(self.p["color2"]) if "color2" in self.p else c1)
                self.stops = [(0, BLACK), (12, BLACK), (25, c1), (38, c1), (50, BLACK),
                              (62, BLACK), (75, c2), (88, c2), (100, BLACK)]
        return gradient(self.stops, now / self.period % 1), 1


class Fire(Layer):
    """Il calore tremola per conto suo su ogni LED: caldo = color, tiepido = color2, freddo = buio."""
    def __init__(self, leds, p, lp):
        super().__init__(leds, p, lp)
        c1 = rgb(p.get("color", 0xFF0000))
        self.stops = [(0, c1), (50, rgb(p["color2"]) if "color2" in p else c1), (100, BLACK)]

    def at(self, now, i):
        heat = (math.sin(now * 7.3 + i * 1.7) + math.sin(now * 12.1 + i * 2.9) + math.sin(now * 3.7 + i * 5.3)) / 6 + 0.5
        return gradient(self.stops, (1 - heat) * 0.8), 1      # 0.8: la fiamma non si spegne mai del tutto


LAYERS = {"static": Static, "spectrum": Spectrum, "wave": Wave, "wheel": Wheel, "tidal": Tidal,
          "ripple": Ripple, "starlight": Starlight, "breathing": Breathing, "fire": Fire}
