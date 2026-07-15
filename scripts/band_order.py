# Zippering order parameter, v4 (final).
# For each magnet pole: locate the pole center from its bright blob (box
# blur + argmax near the nominal position), take the radial intensity
# profile, detrend (5 px smooth minus 31 px envelope), and use the peak of
# the normalized autocorrelation at lags 18..60 px as the ring-order value.
# Report the mean over the hour-tip and minute-tip poles.
# Geometry: time 09:00:00, size 1000, dial center (500,500), radius 470.
import glob
import os
import sys

import numpy as np
from PIL import Image

POLES = [(256.0, 500.0), (500.0, 133.0)]  # hour tip (9h), minute tip (12)
R_MIN, R_MAX = 15, 160

def box_blur(a, k):
    kern = np.ones(k) / k
    a = np.apply_along_axis(np.convolve, 0, a, kern, "same")
    return np.apply_along_axis(np.convolve, 1, a, kern, "same")

def pole_order(img, cx0, cy0):
    # Refine the pole center: brightest blurred point within +-20 px.
    y0, y1 = int(cy0) - 20, int(cy0) + 21
    x0, x1 = int(cx0) - 20, int(cx0) + 21
    win = box_blur(img[y0 - 4 : y1 + 4, x0 - 4 : x1 + 4], 9)[4:-4, 4:-4]
    dy, dx = np.unravel_index(np.argmax(win), win.shape)
    cy, cx = y0 + dy, x0 + dx

    h, w = img.shape
    yy, xx = np.mgrid[0:h, 0:w]
    r = np.hypot(xx - cx, yy - cy)
    mask = (r >= R_MIN) & (r < R_MAX)
    bins = r[mask].astype(int) - R_MIN
    prof = np.bincount(bins, weights=img[mask], minlength=R_MAX - R_MIN)
    cnt = np.bincount(bins, minlength=R_MAX - R_MIN)
    p = prof / np.maximum(cnt, 1)
    density = p.mean()
    p = np.convolve(p, np.ones(5) / 5, "same")
    trend = np.convolve(p, np.ones(31) / 31, "same")
    d = (p - trend)[10:-10]
    if d.std() < 1e-6:
        return 0.0, 0, density
    d = d / d.std()
    n = len(d)
    ac = np.correlate(d, d, "full")[n - 1 :] / n
    lags = np.arange(len(ac))
    k = int(np.argmax(np.where((lags >= 18) & (lags <= 60), ac, -np.inf)))
    return float(ac[k]), k, density

def analyze(path):
    img = np.asarray(Image.open(path).convert("L"), dtype=float)
    results = [pole_order(img, cx, cy) for cx, cy in POLES]
    order = float(np.mean([r[0] for r in results]))
    ring = float(np.mean([r[1] for r in results]))
    dens = float(np.mean([r[2] for r in results]))
    return order, ring, dens

def key(f):
    b = os.path.basename(f)
    return (float(b[2:].split("_")[0]), float(b.split("no")[1][:-4]))

if __name__ == "__main__":
    files = sorted(glob.glob(sys.argv[1] + "/cs*.png"), key=key)
    print(f"{'file':28s} {'order':>7s} {'ring_px':>8s} {'density':>8s}")
    for f in files:
        c, wl, d = analyze(f)
        print(f"{os.path.basename(f):28s} {c:7.3f} {wl:8.1f} {d:8.1f}")
