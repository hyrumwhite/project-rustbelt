# vehicle.py <dir> [from tick]: the ridden vehicle against the camera, frame by frame.
# Reads viewer_trace.txt (V lines with "veh"), game_trace.txt (G lines) and, when there,
# interp_trace.txt (the game's I lines, HALO_INTERPOLATION_TRACE): where the vehicle's root
# sits in the camera's frame (along its forward, sideways), the frames where the along-forward
# offset leaves a running median of its neighbours, and whether the game drew the same jump.
import sys, os, math, statistics as st
from collections import Counter
d = sys.argv[1]
from_tick = int(sys.argv[2]) if len(sys.argv) > 2 else 0
HALF = 7  # neighbours each side in the running median
TOP = 15  # frames listed
REVERSE_STEP, REVERSE_TICKS = 0.08, 4  # a cut's step one way and the other within this many ticks: a pop
def offset(cam, fwd, root):
    # (Halo's z is up: sideways is left, square to forward in the horizontal)
    n = math.sqrt(sum(c * c for c in fwd)) or 1.0
    f = [c / n for c in fwd]
    left = [-f[1], f[0], 0.0]
    m = math.hypot(left[0], left[1]) or 1.0
    left = [c / m for c in left]
    r = [root[i] - cam[i] for i in range(3)]
    return sum(r[i] * f[i] for i in range(3)), sum(r[i] * left[i] for i in range(3))
def deviations(xs):
    out = []
    for i in range(len(xs)):
        w = sorted(xs[max(0, i - HALF):i + HALF + 1])
        out.append(xs[i] - w[len(w) // 2])
    return out
def floats(f, i, n): return [float(x) for x in f[i:i + n]]
def reversals(xs):
    # the ticks whose step is undone by a step the other way within REVERSE_TICKS
    d = [b - a for a, b in zip(xs, xs[1:])]
    return sum(1 for i in range(len(d)) if abs(d[i]) > REVERSE_STEP and any(
        abs(d[j]) > REVERSE_STEP and (d[j] > 0) != (d[i] > 0) for j in range(i + 1, min(len(d), i + 1 + REVERSE_TICKS))))
G = {}
for line in open(os.path.join(d, 'game_trace.txt')):
    f = line.split()
    if len(f) < 4 or f[0] != 'G': continue
    G[int(f[1])] = (int(f[2]), float(f[3]))
# the I lines by frame: fields found by their keywords (a line with no camera or vehicle is shorter)
I = {}
path = os.path.join(d, 'interp_trace.txt')
if os.path.exists(path):
    for line in open(path):
        f = line.split()
        if len(f) < 5 or f[0] != 'I' or 'cam' not in f: continue
        e = dict(idx=int(f[1]), tick=int(f[2]), frac=float(f[3]))
        c = f.index('cam')
        e['branch'] = f[c + 1]
        if e['branch'] != 'none' and len(f) >= c + 11:
            e['obs'], e['cam'], e['fwd'] = floats(f, c + 2, 3), floats(f, c + 5, 3), floats(f, c + 8, 3)
        if 'veh' in f:
            v = f.index('veh')
            e['veh'] = f[v + 1]
            if f[v + 1] != 'none':
                e['state'], e['latest'], e['drawn'] = f[v + 2], floats(f, v + 3, 3), floats(f, v + 6, 3)
        if 'obs' in f:
            o = f.index('obs')
            e['focus'], e['own'], e['timer'] = float(f[o + 1]), float(f[o + 2]), float(f[o + 3])
            e['timers'] = floats(f, o + 4, 5)
            # (the check's own distance, last on the line; older traces have none)
            if len(f) >= o + 10: e['raw'] = float(f[o + 9])
        I[e['idx']] = e
def blend_note(idx):
    e = I.get(idx)
    if not e: return ''
    s = ' | game: %s %s frac %.3f' % (e['branch'], e.get('state', e.get('veh', '-')), e['frac'])
    if 'focus' in e:
        s += ' focus %.3f (own %.3f) timers %s' % (e['focus'], e['own'], ' '.join('%.2f' % t for t in [e['timer']] + e['timers']))
    return s
# (1) the viewer's shown frames
V = []
for line in open(os.path.join(d, 'viewer_trace.txt')):
    f = line.split()
    if len(f) < 12 or f[0] != 'V' or 'veh' not in f: continue
    v = f.index('veh')
    if f[v + 1] == 'none' or len(f) < v + 11: continue
    tick = int(f[3])
    if tick < from_tick: continue
    V.append(dict(now=int(f[1]), idx=int(f[2]), tick=tick, cam=floats(f, 5, 3), fwd=floats(f, 9, 3), veh=f[v + 1], root=floats(f, v + 2, 3)))
if V:
    ridden = Counter(x['veh'] for x in V).most_common(1)[0][0]
    other = sum(1 for x in V if x['veh'] != ridden)
    V = [x for x in V if x['veh'] == ridden]
    for x in V: x['along'], x['side'] = offset(x['cam'], x['fwd'], x['root'])
    along = [x['along'] for x in V]
    dev = deviations(along)
    sdev = deviations([x['side'] for x in V])
    steps = Counter(b['idx'] - a['idx'] for a, b in zip(V, V[1:]))
    print('viewer: %d frames with vehicle %s (%d with another nearest left out), ticks %d to %d' % (len(V), ridden, other, V[0]['tick'], V[-1]['tick']))
    print('  root in the camera frame: along forward median %.3f (p1 %.3f p99 %.3f), sideways median %.3f' % (
        st.median(along), sorted(along)[len(along) // 100], sorted(along)[-1 - len(along) // 100], st.median(x['side'] for x in V)))
    print('  deviation from the running median (%d frames): along sd %.4f, sideways sd %.4f' % (2 * HALF + 1, st.pstdev(dev), st.pstdev(sdev)))
    print('  game frames stepped per viewer frame: %s' % ', '.join('%d: %d' % kv for kv in sorted(steps.items())))
    print('  the largest along-forward deviations (frame, tick, deviation, sideways, game frames stepped in and out):')
    order = sorted(range(len(V)), key=lambda i: -abs(dev[i]))[:TOP]
    for i in sorted(order):
        x = V[i]
        step_in = x['idx'] - V[i - 1]['idx'] if i > 0 else 0
        step_out = V[i + 1]['idx'] - x['idx'] if i + 1 < len(V) else 0
        g = G.get(x['idx'])
        print('    frame %d tick %d%s: %+.4f along, %+.4f sideways, stepped %d/%d%s' % (
            x['idx'], x['tick'], ' frac %.3f' % g[1] if g else '', dev[i], sdev[i], step_in, step_out, blend_note(x['idx'])))
else:
    print('viewer: no V lines with a vehicle%s' % (' from tick %d' % from_tick if from_tick else ''))
if not I:
    print('no interp_trace.txt'); sys.exit()
# (2) the game's branches and states
E = [e for e in sorted(I.values(), key=lambda e: e['idx']) if e['tick'] >= from_tick]
print('game: %d I lines' % len(E))
print('  camera: %s' % ', '.join('%s %d' % kv for kv in Counter(e['branch'] for e in E).most_common()))
print('  vehicle: %s' % ', '.join('%s %d' % kv for kv in Counter(e.get('state', e.get('veh', '-')) for e in E).most_common()))
odd = [e for e in E if e['branch'] not in ('blend', 'none') or e.get('state', 'blend') != 'blend']
seated = [e for e in E if 'state' in e]
print('  frames not blended (the first %d of %d, seated or not):' % (min(len(odd), 40), len(odd)))
for e in odd[:40]:
    print('    frame %d tick %d frac %.3f: camera %s, vehicle %s' % (e['idx'], e['tick'], e['frac'], e['branch'], e.get('state', e.get('veh', '-'))))
# timers: how often each observer timer runs (nonzero), and the focus distance's range
if seated:
    names = ['command', 'position', 'offset', 'distance', 'fov', 'orientation']
    runs = [sum(1 for e in seated if ([e['timer']] + e['timers'])[k] > 0) for k in range(6)]
    print('  seated frames: %d; timers running: %s' % (len(seated), ', '.join('%s %d' % (n, r) for n, r in zip(names, runs))))
    fd = [e['focus'] for e in seated]
    cut = sum(1 for e in seated if e['focus'] < e['own'] - 1e-3)
    print('  focus distance: median %.3f, min %.3f, max %.3f; shortened by the penetration check in %d frames' % (st.median(fd), min(fd), max(fd), cut))
    # the cut's reversals, on the first frame of each tick: the check's own (own - raw) and the placed (own - focus)
    first = {}
    for e in seated: first.setdefault(e['tick'], e)
    ticks = [first[t] for t in sorted(first)]
    raw = [e for e in ticks if 'raw' in e]
    print('  cut reversals (a step over %.2f undone within %d ticks): the check\'s own %s, placed %d (of %d ticks)' % (
        REVERSE_STEP, REVERSE_TICKS, '%d' % reversals([e['own'] - e['raw'] for e in raw]) if raw else 'not in this trace',
        reversals([e['own'] - e['focus'] for e in ticks]), len(ticks)))
# (3) the same offset from the I lines alone: drawn root against drawn camera, every game frame
S = [e for e in seated if 'cam' in e]
if S:
    ridden_game = Counter(e['veh'] for e in S).most_common(1)[0][0]
    S = [e for e in S if e['veh'] == ridden_game]
    for e in S:
        e['along'], e['side'] = offset(e['cam'], e['fwd'], e['drawn'])
        # (the same at the latest tick, from the observer's position: what a frame without the blend would show)
        e['raw_along'], _ = offset(e['obs'], e['fwd'], e['latest'])
    along = [e['along'] for e in S]
    dev = deviations(along)
    by_idx = {e['idx']: k for k, e in enumerate(S)}
    print('game: %d seated frames in vehicle %s: along forward median %.3f, deviation sd %.4f (sideways sd %.4f)' % (
        len(S), ridden_game, st.median(along), st.pstdev(dev), st.pstdev(deviations([e['side'] for e in S]))))
    print('  the largest along-forward deviations of the drawn root from the drawn camera:')
    for k in sorted(sorted(range(len(S)), key=lambda k: -abs(dev[k]))[:TOP]):
        e = S[k]
        print('    frame %d tick %d: %+.4f along (latest tick from the observer %+.3f)%s' % (e['idx'], e['tick'], dev[k], e['raw_along'], blend_note(e['idx'])))
    if V:
        # the viewer's jumps set against the game's at the same frame: a jump the game did not draw is the viewer's
        vdev = deviations([x['along'] for x in V])
        print('  the viewer\'s largest deviations against the game\'s at the same frame:')
        for i in sorted(sorted(range(len(V)), key=lambda i: -abs(vdev[i]))[:TOP]):
            k = by_idx.get(V[i]['idx'])
            print('    frame %d: viewer %+.4f, game %s' % (V[i]['idx'], vdev[i], '%+.4f' % dev[k] if k is not None else 'no line'))
