# slow.py <dir>: the game's work per frame (start of frame to shared), the slow ones by tick
import sys, os
d = sys.argv[1]
G = []
for line in open(os.path.join(d, 'game_trace.txt')):
    f = line.split()
    if len(f) < 19 or f[0] != 'G': continue
    G.append(dict(idx=int(f[1]), tick=int(f[2]), frac=float(f[3]), shared=int(f[5]), end=int(f[10]), wait=int(f[18])))
t1 = G[-1]['end'] - 12e6; t0 = t1 - 30e6
W = []
for a, b in zip(G, G[1:]):
    if t0 <= b['end'] <= t1: W.append((b['shared'] - a['end'], b['tick'], a['tick'], b['idx']))
w = sorted(x[0] for x in W)
n = len(w)
print('%d frames; work median %.2f ms p90 %.2f p99 %.2f max %.2f' % (n, w[n//2]/1e3, w[n*9//10]/1e3, w[n*99//100]/1e3, w[-1]/1e3))
slow = [x for x in W if x[0] > 6000]
print(len(slow), 'over 6 ms; crossing a tick:', sum(1 for x in slow if x[1] != x[2]), '; ticks crossed in all:', sum(1 for x in W if x[1] != x[2]))
print('slow ticks:', ' '.join('%d(%.0f)' % (x[1], x[0]/1e3) for x in slow[:40]))
tk = [x[1] for x in slow]
print('gaps:', ' '.join(str(b - a) for a, b in zip(tk, tk[1:]))[:300])
