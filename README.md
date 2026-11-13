# QuanJP Prime Hunter 2050 — Symbolic-First Edition

**Machine Learning-Guided Prime Number Discovery using Rust & Rayon**

A high-performance Rust application for discovering large probable primes (thousands to tens of thousands of digits) using ML-guided symbolic generation, a multi-stage symbolic-first pipeline, and SQLite-backed historical learning.

---

## Quick Start

```bash
# Build with optimizations
cargo build --release

# Run prime hunter (~20,000 digits by default; pass a sequence length to change it)
./target/release/quanjp-prime-hunter
./target/release/quanjp-prime-hunter 16700   # ~10,053 digits

# Ingest historical results into the learning database
cargo build --release --bin ingest-history
./target/release/ingest-history
```

---

## Recent Discoveries

### 10,000+ Digit Primes (Feb 28, 2026 — all 16 cores)

| # | Digits | Time | Attempts | Rate |
|---|--------|------|----------|------|
| 1 | **10,055** | 1,252s | 38,208 | 30.5/sec |
| 2 | **10,055** | 213s  | 6,764  | 31.7/sec |
| 3 | **10,055** | ~220s | ~7,000 | ~31/sec  |
| 4 | **10,054** | ~230s | ~7,200 | ~31/sec  |

### Previous Records

| Digits | File | Time | Tier | Status |
|--------|------|------|------|--------|
| **9,994** | `quanjp_ultimate_9994digits.txt` | ~700s | Tier 3 | ✅ 10K breakthrough |
| **8,193** | `quanjp_ultimate_8193digits.txt` | 1,394s | Tier 0 | ✅ |
| **8,008** | `quanjp_ultimate_8008digits.txt` | 798s | Tier 3 | ✅ 43% faster |
| **8,007** | `quanjp_ultimate_8007digits.txt` | 518s | Tier 3 | ✅ 63% faster |
| **4,933** | `quanjp_ultimate_4933digits.txt` | 104s | Tier 0 | ✅ |
| **4,931** | `quanjp_ultimate_4931digits.txt` | 197s | Tier 0 | ✅ |
| **2,467** | `quanjp_ultimate_2467digits.txt` | 12s  | Tier 0 | ✅ |

All result files live in `results/`.

---

## Project Structure

```
SPD-Primes/
├── src/
│   ├── main.rs                  # Prime hunter — symbolic-first pipeline
│   └── bin/
│       └── ingest_history.rs    # Ingest results into SQLite DB
├── results/                     # All discovered primes
│   ├── quanjp_prime_NN_*digits.txt   # Batch-run format (current)
│   └── quanjp_ultimate_*digits.txt   # Legacy single-run format
├── docs/                        # Extended documentation & guides
├── verification/                # OpenPFGW input files
├── tools/                       # pfgw64 binary
├── data/                        # prime_history.db (SQLite)
├── Cargo.toml
├── README.md
├── DISCOVERIES.md               # Full discovery log & benchmarks
└── GETTING_STARTED.md           # Step-by-step setup guide
```

---

## How It Works

### 1. Symbolic Seed Generation
- Uses a 4-symbol alphabet: `{-1, 0, 1, 2}` — each symbol is one base-4 digit (`-1` stands for 3)
- A Markov transition matrix learned from previously discovered prime sequences shapes the seed
  (75% learned transitions, 25% random exploration)
- The seed is forced odd and its top symbol non-zero, so it always has the target size

> **Measured, Nov 2026:** in a controlled test (3,000 primes per arm at ~308 digits) the
> pattern-guided generator and plain uniform random digits needed the same number of
> primality tests per prime (118 vs 114, expected 116). The guidance shapes the seed but
> does not raise the hit rate — the speed comes from the sieve below.

### 2. Deep-Sieve Pipeline

```
Symbolic seed → Sieve window → Strong test base 2 → Confirm (Miller-Rabin + Baillie-PSW)
```

| Stage | Operation | Effect |
|-------|-----------|--------|
| ① | Symbolic seed, collapsed to an integer by O(n) bit packing | one seed per hunt |
| ② | Sieve the window `seed, seed+2, seed+4, …` with every prime up to an auto-tuned bound | ~93–95% of odd candidates removed |
| ③ | One strong probable-prime test (base 2) per survivor, all cores | the only expensive step |
| ④ | First survivor to pass: 15 Miller-Rabin rounds + GMP Baillie-PSW, run in parallel | confirmation |

**Why the sieve matters:** every candidate that reaches stage ③ costs a full modular
exponentiation (seconds at 10K digits). Striking it out because a small prime divides it
costs one `seed mod p` (about a microsecond). The sieve bound is chosen per run where those
two costs break even, measured on the machine: about 10⁷ at 1,200 digits, up to the 2³² cap
at 10K+ digits. The fraction of odd numbers that survive a sieve to `B` is `1.123 / ln(B)`,
versus 32.7% for the earlier fixed filter (primes up to 23).

The window holds ~30 expected primes, and survivors are tested in random order.
The symbols saved with each prime are the base-4 digits of the prime itself.

### 3. Proth Mode — Proven Primes from Structured Symbols

```bash
cargo build --release --bin proth-hunter
./target/release/proth-hunter [n_symbols] [how_many_primes]
./target/release/proth-hunter 16700     # one proven ~10,060-digit prime
```

Searches numbers of the form `k·4^n + 1`. In the symbol alphabet that is a `1`, then
`n − 1` zeros, then the short head of `k`. The shape buys two things:

- **The sieve never builds a big number.** A prime `p` divides `k·4^n + 1` exactly when
  `k ≡ −(4^n)⁻¹ (mod p)`, which is word-sized arithmetic on `k` and `n`.
- **The result is a proof.** Proth's theorem: if `a^((N−1)/2) ≡ −1 (mod N)`, then `N` is
  prime. One modular exponentiation, no "probable".

Measured on 4 cores at ~1,233 digits: about 0.5 s per **proven** prime, versus 0.73 s per
probable prime in the general hunter (same cost per test; no confirmation rounds needed).
Profile: 99.5% of the time is the modular exponentiation, 0.5% the sieve.

Results are written as `proth_NN_XXXXdigits.txt` with the form, the witness `a`, the
prime and its symbols.

### 3. Historical Learning Database
- SQLite stores every discovered prime (timestamp, digit count, entropy, symbol sequence, PhaseToken hashes)
- Loaded automatically for targets ≥ 8,000 digits
- Pattern matrix trained on all historical sequences — each new discovery sharpens future guidance

---

## Configuration

Target size and count are command-line arguments — no rebuild needed:

```bash
./target/release/quanjp-prime-hunter [sequence_len] [how_many_primes]

./target/release/quanjp-prime-hunter 16700      # one ~10,053-digit prime
./target/release/quanjp-prime-hunter 8192 10    # ten ~4,932-digit primes
./target/release/quanjp-prime-hunter            # default: one ~20,000-digit prime
```

Digits ≈ `sequence_len × 0.602`.

| Target Digits | Sequence Length |
|---|---|
| ~2,466 | 4,096 |
| ~4,932 | 8,192 |
| ~8,000 | 13,300 |
| ~10,053 | 16,700 |
| ~20,000 | 33,200 |

### Config Parameters (`src/main.rs`)

```rust
struct Config {
    sequence_len: usize,           // Symbol sequence length
    primality_rounds: u32,         // Miller-Rabin confirmation rounds: 15
    pattern_guide_ratio: f64,      // Learned pattern use in the seed: 75%
    num_threads: usize,            // All physical cores (auto-detect)
    window_expected_primes: f64,   // Window size, in expected primes: 30
}
```

---

## Performance

### Deep-sieve pipeline (Nov 2026, measured on a 4-core machine)

| Target | Before | After | Notes |
|--------|--------|-------|-------|
| ~1,233 digits | 2.82 s per prime (546 tests) | ~0.7 s per prime (~107 tests) | 40 primes per run |
| ~10,055 digits | ~3,790 tests per prime expected | 881 tests, 835 s (one run) | sieve bound 9.8×10⁸, 5.40% of odd candidates survive |

At 10K digits the sieve leaves about 625 tests per prime on average, against about 3,790
for the earlier filter — roughly 6× fewer. A single hunt still varies a lot around that
average (the run above needed 881). Sieving took 30 s of the 835 s. Memory use is ~10 MB;
the earlier power-of-4 table held about 5 GB.

The timings in the sections below and in `DISCOVERIES.md` were recorded with the earlier pipeline.

### Threading (16-core system)

Current config uses all 16 cores. Earlier experiments at 4K targets showed 15 cores
(1 reserved) as marginally faster due to OS scheduling. At 10K targets the difference
is negligible — all 16 cores are used.

### Scaling

| Target | Digits | Time | Scaling |
|--------|--------|------|---------|
| 4K | ~4,000 | ~52s | 1× baseline |
| 8K | ~8,008 | ~518s | 10× |
| 10K | ~9,994 | ~700s | 13.5× |
| 10K (16c)| ~10,055 | 213–1,252s | variance-dominated |

Scaling is **linear** with target size — no exponential cliff. RNG variance dominates
wall-clock time at this scale.

### Parallelization
- Rayon thread pool — lock-free candidate generation per thread
- Atomic counters for pipeline stage statistics
- Real-time progress display

---

## PhaseToken — Cryptographic Proof

Every discovery includes a tamper-evident PhaseToken:

```
PhaseToken
==========
Timestamp:    1772305980122
Session Hash: f15579952dd83646...  (SHA3-512 of session ID + timestamp)
Result Hash:  5dad8415119f2d78...  (SHA3-512 of the prime number)
```

Enables reproducible discovery verification and audit trail.

---

## Database

```bash
# Populate from all results/ files
./target/release/ingest-history
```

Schema:

```sql
CREATE TABLE discoveries (
    id INTEGER PRIMARY KEY,
    timestamp INTEGER NOT NULL,
    digits INTEGER NOT NULL,
    entropy REAL NOT NULL,
    sequence_length INTEGER NOT NULL,
    symbols TEXT NOT NULL,       -- JSON array
    session_hash TEXT NOT NULL,
    result_hash TEXT NOT NULL
);
```

---

## Dependencies

| Crate | Purpose |
|-------|---------|
| `rug` | GMP arbitrary-precision integers (Miller-Rabin) |
| `rayon` | Data parallelism across CPU cores |
| `rand` | Random number generation |
| `num-cpus` | Physical/logical core detection |
| `rusqlite` | SQLite database (bundled) |
| `sha3` | SHA3-512 PhaseToken hashing |
| `colored` | Terminal color output |
| `chrono` | Timestamps |
| `serde`/`serde_json` | Serialization |

---

## Verification

```bash
# Extract pfgw binary
cd tools && 7z x pfgw64-4.1.7_linux.7z && cd ..

# Verify a prime
echo "[prime number here]" > verify_input.txt
./tools/pfgw64 verify_input.txt
```

See `docs/OPENPFGW_INSTRUCTIONS.md` and `docs/VALIDATION_GUIDE.md` for full workflow.

---

## Future Work

- [ ] 16K+ digit prime hunt
- [ ] ECPP verification for proven (not just probable) primes
- [ ] GPU acceleration for Miller-Rabin
- [ ] Distributed search across multiple machines
- [ ] Submit to Prime Pages (T5K)

---

## Related Resources

- [Prime Pages (T5K)](https://t5k.org)
- [Miller-Rabin primality test](https://en.wikipedia.org/wiki/Miller%E2%80%93Rabin_primality_test)
- [OpenPFGW](https://sourceforge.net/projects/openpfgw/)
- [GIMPS](https://www.mersenne.org/)

---

**Last Updated:** February 28, 2026
**Current Status:** Active — 4 primes over 10,000 digits discovered
**Hardware:** 16-core system, all cores utilized
