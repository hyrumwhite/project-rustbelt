"""noise.py <dir>...: frame-to-frame noise of a still view's 4-frame series (x_*.png).

Each frame against the mean of its neighbours (cancels slow, steady motion);
the per-pixel mean of that residual over the series, reported as its mean
and p95 over the view (a10's boarding craft: 0.04 good, 0.07 visibly
boiling). Writes noise.png beside the captures. The HUD rows are left out.
"""
import sys, glob, os
import numpy as np
from PIL import Image

for d in sys.argv[1:]:
    fs = sorted(glob.glob(os.path.join(d, 'x_*.png')))
    if len(fs) < 3:
        print(f'{d}: too few captures ({len(fs)})')
        continue
    a = np.stack([np.asarray(Image.open(f).convert('L')).astype(float) for f in fs])[:, 90:600]
    r = a[1:-1] - 0.5 * (a[:-2] + a[2:])
    m = np.abs(r).mean(axis=0)
    print(f'{os.path.basename(d)}: {len(fs)} frames, noise mean {m.mean():.3f} p95 {np.percentile(m, 95):.2f}')
    Image.fromarray(np.clip(m * 12, 0, 255).astype('uint8')).save(os.path.join(d, 'noise.png'))
