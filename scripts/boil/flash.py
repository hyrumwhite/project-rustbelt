"""flash.py <x0,y0,x1,y1> <dir>...: flashes in a box of a long still run (flash.sh's x_*.png).

Per pixel (luminance, a 9-pixel box blur), the excess over its own median in
time, relative to that median (plus 8 of 255, so black doesn't count); pixels
brighter than 35 in the median (an emitter's own glow) are left out. Prints
the box's median, the mean excess and mean dip, the share of pixel-captures
more than 20% and 50% over (in thousandths), the captures with a flash over
50% and over 100% anywhere in the box, and the largest. The strip hall's door
(c20_strips, box 600,0,1110,600): 0.85 thousandths over 50% and 101 captures
of 300 over 100% before the glossy lobe was shared, 0.1 and 8 after.
"""
import sys, glob, os
import numpy as np
from PIL import Image


def box(img, k=4):
    h, w = img.shape
    p = np.pad(img, k, mode='edge')
    c = np.cumsum(np.cumsum(p, 0, dtype=np.float64), 1)
    c = np.pad(c, ((1, 0), (1, 0)))
    return ((c[2*k+1:, 2*k+1:] - c[:-2*k-1, 2*k+1:] - c[2*k+1:, :-2*k-1] + c[:-2*k-1, :-2*k-1])[:h, :w] / (2*k+1) ** 2).astype(np.float32)


x0, y0, x1, y1 = (int(v) for v in sys.argv[1].split(','))
lum = np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
for d in sys.argv[2:]:
    fs = sorted(glob.glob(os.path.join(d, 'x_*.png')))
    st = np.stack([box(np.asarray(Image.open(f).convert('RGB'), dtype=np.float32) @ lum)[y0:y1, x0:x1] for f in fs])
    med = np.median(st, 0)
    keep = med < 35
    ex = (np.maximum(st - med, 0) / (med + 8.0))[:, keep]
    dn = (np.maximum(med - st, 0) / (med + 8.0))[:, keep]
    peak = ex.max(1)
    name = os.path.basename(os.path.dirname(os.path.normpath(d))) if os.path.basename(os.path.normpath(d)) == 'still' else d[-28:]
    print('%-28s %3d caps, median %.1f; excess mean %.2f e-3, dips %.2f e-3, share>0.2 %.2f e-3, >0.5 %.2f e-3; captures with a flash >0.5: %d, >1: %d; peak max %.2f' % (
        name, len(fs), med[keep].mean(), ex.mean() * 1e3, dn.mean() * 1e3, (ex > 0.2).mean() * 1e3, (ex > 0.5).mean() * 1e3, (peak > 0.5).sum(), (peak > 1).sum(), peak.max()))
