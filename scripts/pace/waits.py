# waits.py <dir> [skip_tail_s] [span_s]: render frames whose wait for the window (or whose whole frame) is long
import sys, os, statistics as st
d = sys.argv[1]; tail = float(sys.argv[2]) if len(sys.argv) > 2 else 12.0; span = float(sys.argv[3]) if len(sys.argv) > 3 else 60.0
R = []
for l in open(os.path.join(d, 'viewer_trace.txt.cpu')):
    f = l.split()
    if f[0] == 'R' and len(f) >= 16: R.append((int(f[1]), int(f[2]), int(f[4])))
end = R[-1][0] - tail * 1e6
R = [r for r in R if end - span * 1e6 <= r[0] <= end]
iv = [b[0] - a[0] for a, b in zip(R, R[1:])]
w = [r[2] for r in R]
long = [x for x in iv if x > 1.7 * st.median(iv)]
print('%s: %d render frames in %.0f s (%.1f/s); interval median %.2f ms sd %.2f; wait median %.2f ms; %d intervals over 1.7x the median (%.1f a minute): %s' % (
    os.path.basename(d), len(R), (R[-1][0] - R[0][0]) / 1e6, len(iv) * 1e6 / (R[-1][0] - R[0][0]), st.median(iv) / 1e3, st.pstdev(iv) / 1e3, st.median(w) / 1e3,
    len(long), len(long) * 60e6 / (R[-1][0] - R[0][0]), ' '.join('%.0f' % (x / 1e3) for x in long[:24])))
