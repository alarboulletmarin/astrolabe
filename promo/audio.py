"""The soundtrack, synthesised from nothing: a 120 BPM beat in A minor, key clicks on every typed
character, whooshes into each cut, hits, and an end chime. Reads timeline.json, writes soundtrack.wav."""
import json, pathlib, wave
import numpy as np

HERE = pathlib.Path(__file__).parent
TL = json.loads((HERE / "timeline.json").read_text())
SR, DUR = 48_000, 30.0
N = int(SR * DUR)
BEAT = 60 / TL["bpm"]
rng = np.random.default_rng(42)

L = np.zeros(N); R = np.zeros(N)
send_L = np.zeros(N); send_R = np.zeros(N)  # reverb send


def place(sig, t, gain=1.0, pan=0.0, verb=0.0):
    """Adds a mono signal at time t (seconds), panned -1..1, with some of it to the reverb."""
    i = int(round(t * SR))
    if i >= N: return
    sig = sig[: N - i]
    gl, gr = np.cos((pan + 1) * np.pi / 4), np.sin((pan + 1) * np.pi / 4)
    L[i:i + len(sig)] += sig * gain * gl * 1.414
    R[i:i + len(sig)] += sig * gain * gr * 1.414
    if verb:
        send_L[i:i + len(sig)] += sig * gain * verb * gl
        send_R[i:i + len(sig)] += sig * gain * verb * gr


def ts(d): return np.arange(int(d * SR)) / SR
def hz(note): return 440.0 * 2 ** ((note - 69) / 12)


def onepole_lp(x, fc):
    """One-pole low-pass; fc may be an array (a sweep)."""
    fc = np.broadcast_to(np.asarray(fc, float), x.shape)
    a = np.exp(-2 * np.pi * fc / SR)
    y = np.empty_like(x); s = 0.0
    for i in range(len(x)):
        s = (1 - a[i]) * x[i] + a[i] * s; y[i] = s
    return y


def svf_bp(x, fc, q=2.0):
    """State-variable band-pass with a moving centre frequency."""
    fc = np.broadcast_to(np.asarray(fc, float), x.shape)
    f = 2 * np.sin(np.pi * np.minimum(fc, SR / 6) / SR)
    lo = bp = 0.0; y = np.empty_like(x); damp = 1 / q
    for i in range(len(x)):
        hi = x[i] - lo - damp * bp
        bp += f[i] * hi; lo += f[i] * bp; y[i] = bp
    return y


# ---------- drums ----------
def kick(big=False):
    t = ts(0.7 if big else 0.42)
    f = 46 + 120 * np.exp(-t * 38) + (40 if big else 0) * np.exp(-t * 6)
    ph = 2 * np.pi * np.cumsum(f) / SR
    body = np.sin(ph) * np.exp(-t * (4.2 if big else 7.5))
    click = rng.standard_normal(len(t)) * np.exp(-t * 900) * 0.35
    return np.tanh((body + click) * 1.6) * 0.9


def clap():
    t = ts(0.32)
    n = rng.standard_normal(len(t))
    env = np.zeros_like(t)
    for d in (0, 0.011, 0.022, 0.031):
        env += (t >= d) * np.exp(-(t - d) * 120) * (t >= d)
    env += np.exp(-t * 18) * 0.35
    return svf_bp(n, 1500, 1.4) * env * 1.4


def hat(open_=False):
    t = ts(0.22 if open_ else 0.06)
    n = rng.standard_normal(len(t))
    hp = n - onepole_lp(n, 7000)
    return hp * np.exp(-t * (16 if open_ else 75)) * 0.5


def snare_roll_hit():
    t = ts(0.18)
    return svf_bp(rng.standard_normal(len(t)), 2200, 1.0) * np.exp(-t * 30)


# ---------- tonal ----------
def saw(f, t, detune=0.0):
    out = np.zeros_like(t)
    for d in (-detune, 0, detune):
        ph = (t * f * (1 + d) + rng.random()) % 1.0
        out += 2 * ph - 1
    return out / 3


def pluck(note, d=0.32, bright=1.0):
    t = ts(d)
    f = hz(note)
    tone = np.sin(2 * np.pi * f * t) + 0.45 * np.sin(2 * np.pi * 2 * f * t) * np.exp(-t * 20) * bright + 0.2 * np.sin(2 * np.pi * 3 * f * t) * np.exp(-t * 35) * bright
    return tone * np.exp(-t * 11) * (1 - np.exp(-t * 900)) * 0.35


def bass(note, d):
    t = ts(d)
    f = hz(note)
    s = saw(f, t, 0.004)
    s = onepole_lp(s, 220 + 1600 * np.exp(-t * 18))
    s += 0.6 * np.sin(2 * np.pi * f * t)
    env = (1 - np.exp(-t * 400)) * np.exp(-t * 3) * np.minimum(1, (d - t) * 60)
    return np.tanh(s * env * 1.5) * 0.5


def pad_chord(notes, d, cutoff=900):
    t = ts(d)
    s = sum(saw(hz(n), t, 0.006) for n in notes) / len(notes)
    s = onepole_lp(onepole_lp(s, cutoff), cutoff)
    env = np.minimum(1, t / 0.25) * np.minimum(1, (d - t) / 0.3)
    return s * env * 0.5


def stab(notes, bright=1.0):
    t = ts(0.38)
    s = sum(saw(hz(n), t, 0.01) for n in notes) / len(notes)
    s = onepole_lp(s, 600 + 5000 * bright * np.exp(-t * 14))
    return s * np.exp(-t * 7) * (1 - np.exp(-t * 600)) * 0.6


# ---------- sound design ----------
def keyclick(space=False):
    t = ts(0.05)
    n = rng.standard_normal(len(t))
    thock = np.sin(2 * np.pi * (1900 if not space else 900) * t * (1 + rng.uniform(-0.08, 0.08))) * np.exp(-t * 300)
    tick = (n - onepole_lp(n, 4000)) * np.exp(-t * 520)
    bottom = svf_bp(n, 600 if space else 900, 2) * np.exp(-t * 140) * 0.8
    return (tick * 0.9 + thock * 0.25 + bottom) * 0.5


def whoosh(d, rise=0.8):
    """Noise through a band-pass sweeping up, swelling to the cut and falling off just after."""
    tail = 0.35
    t = ts(d + tail)
    n = rng.standard_normal(len(t))
    u = np.clip(t / d, 0, 1)
    fc = 300 + 5200 * u ** 2.2 - 3000 * np.clip((t - d) / tail, 0, 1)
    s = svf_bp(n, fc, 1.2)
    env = np.where(t < d, u ** (2.5 * rise), np.exp(-(t - d) * 14))
    return s * env * 0.7


def riser(d):
    t = ts(d)
    u = t / d
    n = rng.standard_normal(len(t))
    s = svf_bp(n, 400 + 7000 * u ** 2, 3) * u ** 3
    tone = np.sin(2 * np.pi * np.cumsum(220 + 660 * u ** 2) / SR) * u ** 2 * 0.15
    return (s * 0.5 + tone)


def sub_boom():
    t = ts(1.6)
    f = 34 + 60 * np.exp(-t * 9)
    return np.sin(2 * np.pi * np.cumsum(f) / SR) * np.exp(-t * 2.2) * 0.9


def reverse_cymbal(d):
    t = ts(d)
    n = rng.standard_normal(len(t))
    hp = n - onepole_lp(n, 5000)
    return hp * (t / d) ** 3 * 0.45


def bell(note, d=4.5):
    """FM-ish bell: inharmonic partials with their own decays."""
    t = ts(d)
    f = hz(note)
    parts = [(1, 1, 1.1), (2.0, .5, 1.6), (2.76, .38, 2.4), (5.4, .2, 4), (8.93, .1, 6), (0.5, .25, .9)]
    s = sum(a * np.sin(2 * np.pi * f * r * t + 1.3 * np.sin(2 * np.pi * f * 1.4 * t) * np.exp(-t * 8)) * np.exp(-t * dk) for r, a, dk in parts)
    return s * (1 - np.exp(-t * 2000)) * 0.22


# ---------- arrangement ----------
CHORDS = [  # A minor, F, C, G: one per bar (2 s)
    ([57, 60, 64], 45), ([53, 57, 60], 41), ([55, 60, 64], 48), ([55, 59, 62], 43),
]
ARP = [0, 2, 1, 2, 0, 1, 2, 1]  # index into chord, per 16th... per 8th note pair


def bar_of(t): return int(t // (BEAT * 4)) % 4


# Pad under everything until the end.
for b in range(15):
    t0 = b * 4 * BEAT
    notes, _ = CHORDS[b % 4]
    cutoff = 500 if t0 < 6 else 1400 if t0 < 23 else 2200
    if t0 >= 27: notes = [57, 61, 64, 69]  # A major at the end
    place(pad_chord([n + 12 for n in notes] + [notes[0]], 4 * BEAT + 0.3, cutoff), t0, 0.32 if t0 >= 3 else 0.45, 0, 0.4)

# Intro riser into the first cut.
place(riser(1.5), 1.5, 0.35, 0, 0.3)

# Drums.
sixteenth = BEAT / 4
for i in range(int(DUR / sixteenth)):
    t = i * sixteenth
    beat_i, sub = divmod(i, 4)
    if 3.0 <= t < 6.0:  # half-time build
        if sub == 0 and beat_i % 2 == 0: place(kick(), t, 0.85)
        if sub == 2: place(hat(), t, 0.25, 0.3)
        if t >= 5.0 and sub in (0, 2) and t < 5.5: place(snare_roll_hit(), t, 0.15 + (t - 5) * 0.4, -0.1)
        if t >= 5.5: place(snare_roll_hit(), t, 0.35 + (t - 5.5) * 0.6, 0.1)
    elif 6.0 <= t < 26.5:
        drop = 18.75 <= t < 19.0  # breath before the manifesto
        if drop: continue
        if sub == 0: place(kick(), t, 0.95)
        if sub == 0 and beat_i % 2 == 1: place(clap(), t, 0.5, 0, 0.25)
        if sub == 2: place(hat(open_=True), t, 0.22, 0.25)
        elif sub in (1, 3): place(hat(), t, 0.16 + 0.06 * rng.random(), -0.3)
        if 26.0 <= t and sub % 2 == 0: place(snare_roll_hit(), t, 0.3, 0)

# Bass: eighth notes, root and octave, from 6 s.
for i in range(int(6.0 / (BEAT / 2)), int(26.5 / (BEAT / 2))):
    t = i * BEAT / 2
    if 18.75 <= t < 19.0: continue
    _, root = CHORDS[bar_of(t)]
    note = root + (12 if i % 4 == 3 else 0)
    place(bass(note, BEAT / 2 * 0.9), t, 0.55)
# Half-time bass in the build.
for t in np.arange(3.0, 6.0, BEAT * 2):
    place(bass(CHORDS[bar_of(t)][1], BEAT * 1.8), t, 0.45)

# Arp: 16ths from 10 s, brighter over the wall.
for i in range(int(10.0 / sixteenth), int(26.5 / sixteenth)):
    t = i * sixteenth
    if 19.0 <= t < 23.0 or 18.75 <= t < 19.0: continue
    notes, _ = CHORDS[bar_of(t)]
    n = notes[ARP[i % 8]] + 12 + (12 if (i % 16) in (6, 14) else 0)
    place(pluck(n, bright=1.0 if t > 23 else 0.6), t, 0.32 if t > 23 else 0.24, 0.45 * np.sin(i * 0.7), 0.35)

# Manifesto: a chord stab on each cut.
for k, t in enumerate(TL["cuts"]):
    notes, _ = CHORDS[bar_of(t)]
    place(stab([n + 12 for n in notes], 0.6 + 0.4 * (k % 2)), t, 0.4, -0.2 if k % 2 else 0.2, 0.35)

# Sound design from the shared timeline.
for c in TL["clicks"]:
    jitter = rng.uniform(-0.15, 0.15)
    place(keyclick(c["space"]), c["t"], 0.55 if not c["space"] else 0.45, jitter, 0.08)
for w in TL["words"]:
    place(keyclick(False), w["t"], 0.3, 0.0, 0.15)
    place(pluck(81, 0.15, 0.3), w["t"], 0.07, 0, 0.3)
for k, (a, b) in enumerate(TL["whooshes"]):
    place(whoosh(b - a), a, 0.55, -0.6 if k % 2 else 0.6, 0.25)
for h in TL["hits"]:
    if h in (3.0, 6.0, 19.0, 27.0):
        place(sub_boom(), h, 0.7)
        place(kick(big=True), h, 0.8)
    place(reverse_cymbal(0.6), h - 0.6, 0.25, 0, 0.2)

# End chime: an A major bell arpeggio, the last note landing with the star at 27.5.
ch = TL["chime"]
for k, (note, d) in enumerate([(81, 0.0), (85, 0.07), (88, 0.14), (93, 0.24)]):
    place(bell(note, 4.0), ch + d, 0.55 - k * 0.06, (-0.3, 0.3, -0.1, 0.1)[k], 0.6)
place(bell(69, 4.5), ch, 0.35, 0, 0.5)

# ---------- sidechain: duck pad, bass and arp under each kick ----------
# (approximated on the whole mix bus minus the kicks would need stems; duck the reverb and a gentle bus pump instead)
duck = np.ones(N)
for i in range(int(6.0 / BEAT), int(26.5 / BEAT)):
    t0 = int(i * BEAT * SR); tt = np.arange(int(0.22 * SR)) / SR
    seg = 1 - 0.28 * np.exp(-tt * 18)
    duck[t0:t0 + len(seg)] = np.minimum(duck[t0:t0 + len(seg)], seg)

# ---------- reverb: a stereo, decaying-noise impulse response ----------
def ir(seed, d=2.6):
    r = np.random.default_rng(seed)
    t = np.arange(int(d * SR)) / SR
    x = r.standard_normal(len(t)) * np.exp(-t * 2.7)
    return onepole_lp(x, 5000) * 0.06


def fftconv(a, b):
    n = len(a) + len(b) - 1; m = 1 << (n - 1).bit_length()
    return np.fft.irfft(np.fft.rfft(a, m) * np.fft.rfft(b, m), m)[:N]


L += fftconv(send_L * duck, ir(1)); R += fftconv(send_R * duck, ir(2))

# ---------- master: gentle pump, soft clip, fades, normalise ----------
mix = np.stack([L, R]) * duck
mix = np.tanh(mix * 1.1) / np.tanh(1.1)
fade = np.ones(N)
fade[: int(0.02 * SR)] = np.linspace(0, 1, int(0.02 * SR))
fo = int(0.6 * SR); fade[-fo:] = np.linspace(1, 0, fo) ** 2
mix *= fade
mix /= np.max(np.abs(mix)) / 0.89
pcm = (mix.T * 32767).astype(np.int16)
with wave.open(str(HERE / "soundtrack.wav"), "wb") as w:
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(SR); w.writeframes(pcm.tobytes())
print("ok", pcm.shape)
