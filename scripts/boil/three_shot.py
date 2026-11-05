"""three_shot.py <dir>...: the user's three-shot spread over a series (x_*.png).

For each consecutive triple of captures: (max - min) / (mean + 8) per pixel,
boxed over 19 pixels, its p90 over the view; reported as the mean and max over
the triples. This is the measure that matched the user's perception in motion
(a10's corridor: 0.098 here against 0.103 from the user's own captures).
"""
import sys, glob, os
import numpy as np
from PIL import Image


def spread(files):
    st = np.stack([np.asarray(Image.open(f).convert('RGB'), dtype=np.float32) @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32) for f in files])
    s = (st.max(0) - st.min(0)) / (st.mean(0) + 8)
    k = 9
    p = np.pad(s, k, mode='edge')
    c = np.cumsum(np.cumsum(p, 0), 1)
    b = (c[2*k:, 2*k:] - c[:-2*k, 2*k:] - c[2*k:, :-2*k] + c[:-2*k, :-2*k]) / (2*k) ** 2
    return np.percentile(b[:s.shape[0], :s.shape[1]][110:600, 0:1110], 90)


for d in sys.argv[1:]:
    fs = sorted(glob.glob(os.path.join(d, 'x_*.png')))
    if len(fs) < 3:
        print(f'{d}: too few captures ({len(fs)})')
        continue
    vals = [spread(fs[i:i+3]) for i in range(0, len(fs) - 2, 3)]
    print(f'{os.path.basename(d)}: 3-shot p90 mean {np.mean(vals):.3f} max {np.max(vals):.3f}')
