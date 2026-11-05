"""view_shares.py <png>...: the share of each colour class in a diagnostic view
(red, green, blue, magenta, black, other), over the view without the HUD."""
import sys
import numpy as np
from PIL import Image

for path in sys.argv[1:]:
    a = np.asarray(Image.open(path).convert('RGB'), dtype=np.float32)[110:600, 0:1110]
    r, g, b = a[..., 0], a[..., 1], a[..., 2]
    # (tonemapped: a stored 0 reads about 43, a stored 0.5 about 163)
    hi, lo = 120.0, 70.0
    classes = {
        'magenta': (r > hi) & (b > hi) & (g < lo),
        'red': (r > hi) & (g < lo) & (b < lo),
        'green': (g > hi) & (r < lo) & (b < lo),
        'blue': (b > hi) & (r < lo) & (g < lo),
        'black': (r < lo) & (g < lo) & (b < lo),
    }
    named = np.zeros(r.shape, dtype=bool)
    parts = []
    for name, mask in classes.items():
        parts.append(f'{name} {mask.mean():.3f}')
        named |= mask
    parts.append(f'other {(~named).mean():.3f}')
    name = path.replace(chr(92), '/').split('/')[-1]
    if 'KEY_PROBE' in name or 'NORMAL_KEY' in name:
        # (grey codes, read back through the tonemapper by a calibration strip)
        full = np.asarray(Image.open(path).convert('RGB'), dtype=np.float32)
        # (the strip runs down the left edge, 0 at the top to 1 at the bottom;
        # the panels and overlays leave rows 110 to 600 clean, and a monotonic
        # cleanup drops whatever still drew over it)
        strip = np.maximum.accumulate(full[:, 2:6, :].mean(axis=(1, 2)))
        height = strip.shape[0]
        rows = np.arange(110, 600)
        grey = a[:, 8:].mean(axis=2).ravel()
        row = rows[np.abs(grey[:, None] - strip[None, rows]).argmin(axis=1)]
        value = (row + 0.5) / height
        levels = 32 if 'KEY_PROBE' in name else 64
        codes = np.floor((value - 0.15) / 0.65 * levels).clip(0, levels - 1).astype(int)
        values, counts = np.unique(codes, return_counts=True)
        order = np.argsort(-counts)[:8]

        def label(code):
            if 'KEY_PROBE' in name:
                if code == 27:
                    return 'own key'
                if code >= 28:
                    return 'none'
                return f'offset ({code % 3 - 1},{code // 3 % 3 - 1},{code // 9 - 1})'
            return f'key ({code % 4 - 2},{code // 4 % 4 - 2},{code // 16 - 2})'
        print(f"{name}: " + ', '.join(f"{label(int(values[i]))} {counts[i] / codes.size:.3f}" for i in order))
        continue
    if 'DI_PICK' in name:
        # (the channels are shares, not classes: the tile candidates with any
        # contribution, the cell candidates with any, and the pick dropped)
        live = lambda c: ((c - 43.0) / 120.0).clip(0.0, 1.0)
        print(f"{name}: tile candidates live {live(r).mean():.3f} (none {(r < 50).mean():.3f}), "
              f"cell candidates live {live(g).mean():.3f} (none {(g < 50).mean():.3f}), pick dropped {(b > 150).mean():.3f}")
        continue
    print(f"{name}: " + ', '.join(parts))
