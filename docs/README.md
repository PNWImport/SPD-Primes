# QuanJP Prime Hunter — Documentation Index

## Quick Links

| Document | Purpose |
|----------|---------|
| `../README.md` | Project overview, quick start, architecture |
| `../GETTING_STARTED.md` | Step-by-step setup and usage guide |
| `../DISCOVERIES.md` | Full discovery log, benchmarks, pipeline analysis |
| `OPENPFGW_INSTRUCTIONS.md` | Verifying primes with OpenPFGW |
| `VALIDATION_GUIDE.md` | Comprehensive validation workflow |
| `VERIFICATION_EXPECTED_OUTPUT.md` | What a successful verification looks like |
| `R730_DEPLOYMENT.md` | Deployment guide for Dell R730 server |
| `GPU_ACCELERATION.md` | Notes on future GPU (CUDA) acceleration |
| `TEMPLATE_verify_4931.log` | Example OpenPFGW verification log |

---

## Recent Discoveries (Feb 28, 2026)

**4 primes over 10,000 digits** — all in `results/`:

```
results/quanjp_prime_01_10055digits.txt   10,055 digits
results/quanjp_prime_02_10055digits.txt   10,055 digits
results/quanjp_prime_03_10055digits.txt   10,055 digits
results/quanjp_prime_04_10054digits.txt   10,054 digits
```

Plus 10 primes at ~4,932 digits from the same session, and 14 legacy discoveries
going back to 1,987 digits. **28 total primes discovered.**

---

## Algorithm Overview

### Symbolic-First Pipeline (Tier 3)

```
Symbolic Generation
      ↓
 Score Gate  ──── reject ~50%
      ↓
Residue Filters ── reject ~60% of remaining  (mod 3,5,7,11,13,17,19,23)
      ↓
Partial Collapse ─ reject ~25% (parity + mod 65537)
      ↓
Full GMP Collapse  ← only ~12% of all candidates reach here
      ↓
Miller-Rabin (15 rounds)
      ↓
   PRIME ✅
```

### Key Achievement

Only ~12% of candidates require a full GMP big-integer construction. The rest are
eliminated by cheap symbolic operations — this is what makes 10K+ digit searches
tractable on a 16-core workstation.

---

## Verification Workflow

1. Extract `tools/pfgw64-4.1.7_linux.7z`
2. Copy the prime number from a result file into an input file
3. Run `./pfgw64 input.txt`
4. Expect: `PRP! (Probable prime)`

See `OPENPFGW_INSTRUCTIONS.md` for the exact commands.

---

## Hardware

| Component | Spec |
|-----------|------|
| CPU | 16 physical cores (all utilized) |
| RAM | 16GB+ recommended |
| Storage | NVMe SSD (for SQLite DB) |
| OS | Linux |

---

**Last Updated:** February 28, 2026
