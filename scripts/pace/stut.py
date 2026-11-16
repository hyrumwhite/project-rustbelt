# stut.py <dir> [last seconds]: joins the viewer's and the game's frame traces.
import sys, os, statistics as st
d = sys.argv[1]
last = float(sys.argv[2]) if len(sys.argv) > 2 else 30.0
def unwrap(xs):
    out, add, prev = [], 0, None
    for x in xs:
        if prev is not None and x < prev - 2**31: add += 2**32
        prev = x; out.append(x + add)
    return out
G = {}
gl = []
for line in open(os.path.join(d, 'game_trace.txt')):
    f = line.split()
    if len(f) < 19 or f[0] != "G": continue
    idx, tick, frac = int(f[1]), int(f[2]), float(f[3])
    shared, sw0, sw1, end = int(f[5]), int(f[7]), int(f[8]), int(f[10])
    gl.append(dict(idx=idx, t=tick + frac, shared=shared, swap=sw0 - sw1, end=end, looked=int(f[12]), interval=int(f[14]), lead=int(f[16]), wait=int(f[18])))
V = []
for line in open(os.path.join(d, 'viewer_trace.txt')):
    f = line.split()
    if len(f) < 4 or f[0] != 'V': continue
    V.append(dict(now=int(f[1]), idx=int(f[2]), tick=int(f[3]), fwd=(float(f[9]), float(f[10]), float(f[11]))))
if not V or not gl: print('no trace'); sys.exit()
for g in gl: G[g['idx']] = g
t_end = V[-1]['now']
V = [v for v in V if (t_end - v['now']) % 2**32 < last * 1e6 and v['idx'] in G]
vt = unwrap([v['now'] for v in V])
dv = [b - a for a, b in zip(vt, vt[1:])]
def pct(xs, p): xs = sorted(xs); return xs[min(len(xs) - 1, int(p * len(xs)))]
print('viewer: %d frames, %.1f/s; interval mean %.2f ms, sd %.2f, p1 %.2f p50 %.2f p99 %.2f max %.2f' % (
    len(V), 1e6 * len(dv) / (vt[-1] - vt[0]), st.mean(dv) / 1e3, st.pstdev(dv) / 1e3, pct(dv, .01) / 1e3, pct(dv, .5) / 1e3, pct(dv, .99) / 1e3, max(dv) / 1e3))
step = [b['idx'] - a['idx'] for a, b in zip(V, V[1:])]
n = len(step)
print('game frame step per viewer frame: 0 (repeat) %.1f%%, 1 %.1f%%, 2 %.1f%%, 3+ %.1f%%' % tuple(100.0 * sum(1 for s in step if c(s)) / n for c in (lambda s: s == 0, lambda s: s == 1, lambda s: s == 2, lambda s: s >= 3)))
# game time shown against the viewer's time: the error of the shown game time about a line of slope 1 (30 ticks/s)
gt = [G[v['idx']]['t'] / 30.0 * 1e6 for v in V]
err = [g - t for g, t in zip(gt, vt)]
# remove slow drift: residual about a running median of 61 frames
res = []
for i in range(len(err)):
    w = sorted(err[max(0, i - 30):i + 31]); res.append(err[i] - w[len(w) // 2])
print('shown game time against viewer time: residual sd %.2f ms, p1 %.2f, p99 %.2f (of 0: perfectly even motion)' % (st.pstdev(res) / 1e3, pct(res, .01) / 1e3, pct(res, .99) / 1e3))
# the step of game time per viewer frame against the viewer's interval
dg = [b - a for a, b in zip(gt, gt[1:])]
ratio = [(g - v) / 1e3 for g, v in zip(dg, dv)]
print('game-time step minus viewer interval per frame: sd %.2f ms, p1 %.2f, p99 %.2f' % (st.pstdev(ratio), pct(ratio, .01), pct(ratio, .99)))
# age: viewer read time minus the game's share time of the frame shown
gs = {g['idx']: g for g in gl}
age = [((v['now'] - G[v['idx']]['shared']) + 2**31) % 2**32 - 2**31 for v in V]
print('age of the shown frame at the look (look - shared): mean %.2f ms, sd %.2f, p1 %.2f, p99 %.2f' % (st.mean(age) / 1e3, st.pstdev(age) / 1e3, pct(age, .01) / 1e3, pct(age, .99) / 1e3))
# the game's side over the same span
first, lastidx = V[0]['idx'], V[-1]['idx']
gg = [g for g in gl if first <= g['idx'] <= lastidx]
ge = unwrap([g['end'] for g in gg])
dgf = [b - a for a, b in zip(ge, ge[1:])]
print('game: %d frames, %.1f/s; interval mean %.2f ms, sd %.2f, p1 %.2f p99 %.2f max %.2f; swap mean %.2f ms p99 %.2f; wait mean %.2f; lead mean %.2f; viewer interval (game sees) mean %.2f' % (
    len(gg), 1e6 * len(dgf) / (ge[-1] - ge[0]), st.mean(dgf) / 1e3, st.pstdev(dgf) / 1e3, pct(dgf, .01) / 1e3, pct(dgf, .99) / 1e3, max(dgf) / 1e3,
    st.mean([g['swap'] for g in gg]) / 1e3, pct([g['swap'] for g in gg], .99) / 1e3, st.mean([g['wait'] for g in gg]) / 1e3, st.mean([g['lead'] for g in gg]) / 1e3, st.mean([g['interval'] for g in gg]) / 1e3))
foc = [int(l.split()[-1]) for l in open(os.path.join(d, 'viewer_trace.txt')) if ' focus ' in l]
if foc: print('window focused in %.0f%% of the viewer frames (last: %d)' % (100.0 * sum(foc) / len(foc), foc[-1]))
