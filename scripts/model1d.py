# 1D radial reduction of the band-formation dynamics
# (research-chain-banding.md, mechanism synthesis). Needs numpy.
#
# Particles on a radial ray from a single charge pole, with the sim's exact
# force laws: capped field drift, chain pair attraction (floor/range,
# (r_rep/d)^4 falloff, summed-speed cap), soft-core repulsion, per-step
# noise. Reproduces the 2D band spacing (~30-45 px) and its parameter
# insensitivity; the selected scale is the tidal-fragmentation balance
#   delta* ~ (2*cs*r_rep^4 / (mu*|dv/dr|))^(1/5)
# whose fifth root makes every parameter enter with exponent <= 0.2.
#
#   python3 scripts/model1d.py            # baseline + parameter sweep
import numpy as np

def run1d(N=400, T=150.0, dt=1/120, seed=1, mu=2e-8, q=0.577, cs=0.06,
          r_rep=0.012, rep=0.025, floor=0.0096, rng_=0.0228, cap=0.12,
          vmax=0.05, eta=0.008, r_in=0.05, r_out=0.50):
    rng = np.random.default_rng(seed)
    pos = np.sort(rng.uniform(r_in, r_out, N))
    for _ in range(int(T / dt)):
        pos.sort()
        v_chain = np.zeros(N)
        v_rep = np.zeros(N)
        for k in range(1, 40):
            d = pos[k:] - pos[:-k]
            m = (d > floor) & (d < rng_)
            f = np.where(m, 2*cs*(r_rep/np.maximum(d, 1e-9))**4, 0.0)
            v_chain[:-k] += f
            v_chain[k:] -= f
            fr = np.where(d < r_rep, rep*(1 - d/r_rep), 0.0)
            v_rep[:-k] -= fr
            v_rep[k:] += fr
        np.clip(v_chain, -cap, cap, out=v_chain)
        drift = -mu * 4*q*q / np.maximum(pos, 1e-3)**5
        np.clip(drift, -vmax, vmax, out=drift)
        pos += (drift + v_chain + v_rep + eta*rng.choice([-1.0, 1.0], N)) * dt
        pos = np.clip(pos, r_in, r_out)
    return pos

def cluster_spacing(pos, zone=(0.08, 0.36), gap=0.02):
    p = np.sort(pos)
    groups = np.split(p, np.where(np.diff(p) > gap)[0] + 1)
    centers = np.array([g.mean() for g in groups if len(g) >= 3])
    centers = centers[(centers > zone[0]) & (centers < zone[1])]
    return np.diff(centers) * 470  # px at size-1000 rendering

if __name__ == "__main__":
    print("1D model cluster spacing, px (3 seeds pooled)")
    for tag, kw in [("baseline", {}), ("cs x4", {"cs": 0.24}),
                    ("cs /4", {"cs": 0.015}), ("mobility x4", {"mu": 8e-8}),
                    ("noise x4", {"eta": 0.032}), ("N/2", {"N": 200}),
                    ("range x2", {"rng_": 0.0456})]:
        sp = []
        for s in [1, 2, 3]:
            sp += list(cluster_spacing(run1d(seed=s, **kw)))
        a = np.array(sp)
        out = f"median {np.median(a):5.1f}  n={len(a)}" if len(a) else "no clusters"
        print(f"  {tag:12s} {out}")
