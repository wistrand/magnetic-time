# Ordering-front tracker (research-chain-banding.md, experiment 3).
# Needs numpy + PIL.
#
# For each dump (standard geometry: time 09:00:00, size 1000, hour-tip pole
# at 256,500): compute the radial profile around the pole, detrend, and
# measure local band contrast c(r) = smoothed |oscillation| / local density
# in sliding radial windows. The ordering front is the outermost radius
# where c(r) exceeds a threshold; calibrate the threshold against a
# chain-strength-0 control (gas baseline) at the same age.
#
#   python3 scripts/front_track.py THRESHOLD dump1.png [dump2.png ...]
#   python3 scripts/front_track.py --contrast dump.png   # print c(r) curve
#   python3 scripts/front_track.py --peaks dump1.png ... # ring radii per dump
import sys

import numpy as np
from PIL import Image

POLE = (256.0, 500.0)
R_MIN, R_MAX = 15, 210

def contrast_curve(path):
    img = np.asarray(Image.open(path).convert("L"), dtype=float)
    h, w = img.shape
    yy, xx = np.mgrid[0:h, 0:w]
    r = np.hypot(xx - POLE[0], yy - POLE[1])
    mask = (r >= R_MIN) & (r < R_MAX)
    bins = r[mask].astype(int) - R_MIN
    prof = np.bincount(bins, weights=img[mask], minlength=R_MAX - R_MIN)
    cnt = np.bincount(bins, minlength=R_MAX - R_MIN)
    p = prof / np.maximum(cnt, 1)
    p = np.convolve(p, np.ones(5) / 5, "same")
    trend = np.convolve(p, np.ones(31) / 31, "same")
    osc = np.abs(p - trend)
    amp = np.convolve(osc, np.ones(21) / 21, "same")
    c = amp / np.maximum(trend, 5.0)
    radii = np.arange(R_MIN, R_MAX)
    # Edges of the smoothing kernels are unreliable.
    return radii[16:-16], c[16:-16]

def front(path, threshold):
    radii, c = contrast_curve(path)
    # r > 175 is contaminated by the hub-pole ring system; r < 50 by the
    # pole blob edge (calibrated against the chain-strength-0 control).
    valid = radii <= 175
    above = (c > threshold) & valid
    if not above.any():
        return 0
    return int(radii[np.where(above)[0][-1]])

def ring_peaks(path):
    """Radii of detected ring crests (local maxima of the detrended radial
    profile), for tracking ring migration and coarsening over a time series."""
    img = np.asarray(Image.open(path).convert("L"), dtype=float)
    h, w = img.shape
    yy, xx = np.mgrid[0:h, 0:w]
    r = np.hypot(xx - POLE[0], yy - POLE[1])
    mask = (r >= R_MIN) & (r < R_MAX)
    bins = r[mask].astype(int) - R_MIN
    prof = np.bincount(bins, weights=img[mask], minlength=R_MAX - R_MIN)
    cnt = np.bincount(bins, minlength=R_MAX - R_MIN)
    p = prof / np.maximum(cnt, 1)
    p = np.convolve(p, np.ones(7) / 7, "same")
    trend = np.convolve(p, np.ones(41) / 41, "same")
    d = p - trend
    radii = np.arange(R_MIN, R_MAX)
    return [
        int(radii[i])
        for i in range(22, len(d) - 22)
        if d[i] > 3 and d[i] == d[max(0, i - 4) : i + 5].max() and radii[i] <= 175
    ]

if __name__ == "__main__":
    if sys.argv[1] == "--peaks":
        for f in sys.argv[2:]:
            pk = ring_peaks(f)
            print(f"{f.split('/')[-1]:24s} n={len(pk):2d}  radii={pk}")
    elif sys.argv[1] == "--contrast":
        radii, c = contrast_curve(sys.argv[2])
        for r, v in zip(radii[::8], c[::8]):
            print(f"r={r:3d}  c={v:.3f}")
    else:
        thr = float(sys.argv[1])
        print(f"{'file':28s} {'front_px':>9s}")
        for f in sys.argv[2:]:
            print(f"{f.split('/')[-1]:28s} {front(f, thr):9d}")
