# Ghost-decay metric (research-chain-banding.md, experiment 6).
# Needs numpy + PIL.
#
# Compares the broad radial density profile around the hour-tip pole between
# an annealed run (bands formed then erased) and a same-seed control, both
# from the experiment's standard geometry: time 09:00:00, size 1000, hour
# bar at 9 o'clock (pole at 256,500). Ghost = normalized RMS profile
# difference; seed-only pairs give the no-memory baseline (~0.04).
#
#   python3 scripts/ghost_decay.py ann_45.png ctl_45.png [ann_90.png ctl_90.png ...]
import sys

import numpy as np
from PIL import Image

POLE = (256.0, 500.0)
R_MIN, R_MAX = 20, 170

def radial_profile(path):
    img = np.asarray(Image.open(path).convert("L"), dtype=float)
    h, w = img.shape
    yy, xx = np.mgrid[0:h, 0:w]
    r = np.hypot(xx - POLE[0], yy - POLE[1])
    mask = (r >= R_MIN) & (r < R_MAX)
    bins = r[mask].astype(int) - R_MIN
    prof = np.bincount(bins, weights=img[mask], minlength=R_MAX - R_MIN)
    cnt = np.bincount(bins, minlength=R_MAX - R_MIN)
    p = prof / np.maximum(cnt, 1)
    # Broad features only: bands/stroke texture smoothed away.
    return np.convolve(p, np.ones(15) / 15, "same")[8:-8]

def halo_contrast(p):
    """Depleted-halo depth: inner annulus density deficit vs outer."""
    return 1 - p[10:60].mean() / p[85:135].mean()

if __name__ == "__main__":
    args = sys.argv[1:]
    if len(args) < 2 or len(args) % 2 != 0:
        sys.exit("usage: ghost_decay.py ANNEALED.png CONTROL.png [more pairs...]")
    print(f"{'annealed':32s} {'ghost_rms':>10s} {'halo_ann':>9s} {'halo_ctl':>9s}")
    for ann, ctl in zip(args[0::2], args[1::2]):
        pa, pc = radial_profile(ann), radial_profile(ctl)
        ghost = float(np.sqrt(np.mean((pa - pc) ** 2)) / pc.mean())
        name = ann.split("/")[-1]
        print(f"{name:32s} {ghost:10.3f} {halo_contrast(pa):9.3f} {halo_contrast(pc):9.3f}")
