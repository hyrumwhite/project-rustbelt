# press_replay.py <dir> [from tick]: the camera's obstruction check of a traced drive (interp_trace.txt,
# the I lines' own, checked and placed focus distances) run again through the release's ease
# (observer_ease_press in opence/source/camera/observer.c) under other settings: what the placed distance
# would have been, the pops left (the cut let out, then a step back in of over 0.05 within 12 frames: the
# tick, how far out, the step back) and how far the camera is held in under the check. The frame's time is
# taken from the tick and its fraction, so the built rule's replay is near the recorded placed distance,
# not equal to it (the difference is printed). A trace from before the ease has no checked distance of
# its own and the placed one is used.
import sys
def load(d, lo):
    rows = []
    for line in open(d + "/interp_trace.txt"):
        f = line.split()
        if not f or f[0] != "I" or "obs" not in f: continue
        tick = int(f[2])
        if tick < lo: continue
        o = f.index("obs")
        focus, own = float(f[o + 1]), float(f[o + 2])
        raw = float(f[o + 9]) if len(f) >= o + 10 else focus
        if own <= 0: continue
        rows.append((tick, tick + float(f[3]), own, raw, focus))
    return rows
def ease(rows, hold, rate, jump=0.0, ramp=0.0):
    held, timer, prev, speed = 0.0, 0.0, None, 0.0
    out = []
    pt = None
    for tick, t, own, raw, focus in rows:
        dt = 0.0 if pt is None else max(0.0, (t - pt) / 30.0)
        pt = t
        short = max(0.0, own - raw)
        if short >= held:
            held, timer, speed = short, hold, 0.0
        else:
            if jump > 0 and prev is not None and prev - short > jump:
                timer, speed = hold, 0.0
            el = dt
            if timer > 0:
                used = min(timer, el); timer -= used; el -= used
                if timer < 0.001: timer = 0.0
            if timer == 0.0:
                if ramp > 0:
                    speed = min(rate, speed + ramp * el)
                    held = max(held - speed * el, short)
                else:
                    held = max(held - rate * el, short)
        prev = short
        held = min(held, own)
        out.append(own - held)
    return out
def pops(rows, placed):
    cut = [r[2] - p for r, p in zip(rows, placed)]
    s = [cut[i] - cut[i - 1] for i in range(1, len(cut))]
    found = []; i = 0
    while i < len(s):
        if s[i] < -0.03:
            # out over the next frames, then a step back in of over 0.05 within 12 frames
            j = next((j for j in range(i + 1, min(i + 13, len(s))) if s[j] > 0.05), None)
            if j is not None:
                found.append((rows[i + 1][0], round(cut[i] - min(cut[i:j + 1]), 2), round(s[j], 2))); i = j
        i += 1
    return found
d, lo = sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 3300
rows = load(d, lo)
raw = [r[3] for r in rows]
print(d.split("/")[-1], "frames", len(rows))
print("  the check itself: pops", pops(rows, raw))
rules = [("hold 0.1 rate 4 (built)", dict(hold=0.1, rate=4.0)),
         ("+ jump 0.12 restarts the hold", dict(hold=0.1, rate=4.0, jump=0.12)),
         ("+ jump 0.08", dict(hold=0.1, rate=4.0, jump=0.08)),
         ("ramp 16", dict(hold=0.1, rate=4.0, ramp=16.0)),
         ("hold 0.2", dict(hold=0.2, rate=4.0))]
for name, kw in rules:
    p = ease(rows, **kw)
    lag = [r[3] - x for r, x in zip(rows, p)]
    line = "  %-30s pops %s; held in under the check: mean %.4f, over 0.05 in %d frames, largest %.2f" % (name, pops(rows, p), sum(lag) / len(lag), sum(1 for x in lag if x > 0.05), max(lag))
    if name.endswith("(built)") and any(abs(r[4] - r[3]) > 1e-4 for r in rows):
        line += "; model against the recorded placed: largest difference %.3f" % max(abs(r[4] - x) for r, x in zip(rows, p))
    print(line)
