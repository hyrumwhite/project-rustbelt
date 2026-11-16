# gpuh.py <dir>: the GPU pass trace: the sum's spread, and the frames whose sum is far over the median with their long passes
import sys, os, statistics as st, collections
d = sys.argv[1]
P = []
for l in open(os.path.join(d, 'viewer_trace.txt.gpu')):
    f = l.split()
    if f[0] != 'P' or len(f) < 3: continue
    P.append((int(f[1]), float(f[2]), dict((x.split('=')[0], float(x.split('=')[1])) for x in f[3:])))
end = P[-1][0] - 12e6
P = [p for p in P if end - 45e6 <= p[0] <= end]
tot = [p[1] for p in P]
med = st.median(tot)
print('%d frames; gpu sum median %.2f ms, p99 %.2f, max %.2f' % (len(P), med, sorted(tot)[int(len(tot) * .99)], max(tot)))
meds = collections.defaultdict(list)
for p in P:
    for k, v in p[2].items(): meds[k].append(v)
meds = {k: st.median(v) for k, v in meds.items()}
n = 0
for i, p in enumerate(P):
    if p[1] > med * 1.6:
        n += 1
        if n <= 14:
            big = ', '.join('%s %.1f (%.1f)' % (k.split('/')[-1], v, meds.get(k, 0)) for k, v in sorted(p[2].items(), key=lambda kv: -(kv[1] - meds.get(kv[0], 0)))[:4] if v - meds.get(k, 0) > 1.0)
            print('  %.2f s: sum %.1f ms: %s' % ((p[0] - P[0][0]) / 1e6, p[1], big))
print(n, 'frames over 1.6x the median')
