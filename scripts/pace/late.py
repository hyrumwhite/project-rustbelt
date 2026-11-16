# late.py <dir>: what the overlay shared each frame (the G line's last field), in the measured span
import sys, os, collections
c = collections.Counter(); G = []
for line in open(os.path.join(sys.argv[1], 'game_trace.txt')):
    f = line.split()
    if len(f) >= 21 and f[0] == 'G' and f[-2] == 'late': G.append((int(f[10]), f[-1]))
if G:
    t1 = G[-1][0] - 12e6
    for t, l in G:
        if t1 - 30e6 <= t <= t1: c[l] += 1
n = sum(c.values()) or 1
print('late:', ', '.join('%s %.1f%%' % (k, 100.0 * v / n) for k, v in sorted(c.items())))
