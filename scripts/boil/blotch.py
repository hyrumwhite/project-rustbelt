"""blotch.py <dir>...: patch-scale temporal wander of a still view's captures (x_*.png,
40 frames apart or so).

Each capture is blurred over 19-pixel boxes; then per pixel the std over the
captures relative to the mean (+8 so black doesn't dominate), reported as mean,
p90 and p99 over the view, with the same for consecutive captures' change
(|a-b| relative). The HUD rows and the stats panel are masked.
"""
import sys, glob, os
import numpy as np
from PIL import Image


def box(img, k=9):
    h, w = img.shape
    p = np.pad(img, k, mode='edge')
    c = np.cumsum(np.cumsum(p, 0), 1)
    c = np.pad(c, ((1, 0), (1, 0)))
    return (c[2*k+1:, 2*k+1:] - c[:-2*k-1, 2*k+1:] - c[2*k+1:, :-2*k-1] + c[:-2*k-1, :-2*k-1])[:h, :w] / (2*k+1) ** 2


for d in sys.argv[1:]:
    fs = sorted(glob.glob(os.path.join(d, 'x_*.png')))
    if len(fs) < 3:
        print(f'{d}: too few captures ({len(fs)})')
        continue
    ims = []
    for f in fs:
        im = np.asarray(Image.open(f).convert('RGB'), dtype=np.float32) @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
        ims.append(box(im[110:600, 0:1110]))
    st = np.stack(ims)
    mean = st.mean(0)
    rel = st.std(0) / (mean + 8.0)
    step = (np.abs(st[1:] - st[:-1]).mean(0)) / (mean + 8.0)
    print(f'{os.path.basename(d)}: {len(fs)} captures, brightness {mean.mean():.1f}; '
          f'wander mean {rel.mean():.3f} p90 {np.percentile(rel, 90):.3f} p99 {np.percentile(rel, 99):.3f}; '
          f'step mean {step.mean():.3f} p90 {np.percentile(step, 90):.3f}')
