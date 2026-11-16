# rep.py <dir> [n]: where the viewer repeats a game frame or steps 3+, in the measured span; the timeline round the first n
import sys, os
d = sys.argv[1]; n = int(sys.argv[2]) if len(sys.argv) > 2 else 2
G = []
for line in open(os.path.join(d, 'game_trace.txt')):
    f = line.split()
    if len(f) < 19 or f[0] != 'G': continue
    G.append(dict(idx=int(f[1]), tick=int(f[2]), frac=float(f[3]), shared=int(f[5]), end=int(f[10]), wait=int(f[18]), lead=int(f[16]), interval=int(f[14])))
V = []
for line in open(os.path.join(d, 'viewer_trace.txt')):
    f = line.split()
    if f[0] == 'V': V.append((int(f[1]), int(f[2])))
t1 = V[-1][0] - 12e6; t0 = t1 - 30e6
ev = [(g['end'] - g['wait'], 'G  swapped idx %d (work %d us since start; shared %d us before); then waits %d (lead %d)' % (g['idx'], 0, g['end'] - g['wait'] - g['shared'], g['wait'], g['lead'])) for g in G]
ev += [(g['shared'], 'G shared idx %d t %d+%.3f' % (g['idx'], g['tick'], g['frac'])) for g in G]
ev += [(g['end'], 'G start of the next frame') for g in G]
Vs = [v for v in V if t0 <= v[0] <= t1]
bad = []
for a, b in zip(Vs, Vs[1:]):
    st = b[1] - a[1]
    ev.append((b[0], 'V look shows %d (step %d)%s' % (b[1], st, '   <<<<' if st == 0 or st >= 3 else '')))
    if st == 0 or st >= 3: bad.append((b[0], st))
ev.sort()
print(len(Vs), 'looks;', sum(1 for b in bad if b[1] == 0), 'repeats,', sum(1 for b in bad if b[1] >= 3), 'steps of 3+')
# how they cluster in time
import collections
sec = collections.Counter(int((b[0] - t0) / 1e6) for b in bad)
print('per second:', ' '.join(str(sec.get(i, 0)) for i in range(30)))
shown = 0
last = -1e9
for b in bad:
    if b[0] - last < 300000: continue
    last = b[0]
    i = next(k for k, e in enumerate(ev) if e[0] == b[0] and e[1].startswith('V'))
    prev = ev[max(0, i - 16)][0]
    for t, s in ev[max(0, i - 16):i + 10]:
        print('%7d (+%5d) %s' % (t % 1000000, t - prev, s)); prev = t
    print()
    shown += 1
    if shown >= n: break
