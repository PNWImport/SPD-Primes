# QuanJP Prime Hunter 2050 — Symbolic-First Edition

**Machine Learning-Guided Prime Number Discovery using Rust & Rayon**

A high-performance Rust application for discovering large probable primes (thousands to tens of thousands of digits) using ML-guided symbolic generation, a multi-stage symbolic-first pipeline, and SQLite-backed historical learning.

---

## Quick Start

```bash
# Build with optimizations
cargo build --release

# Run prime hunter (10,000+ digit target by default)
./target/release/quanjp-prime-hunter

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

### 1. ML-Guided Symbolic Generation
- Learns a Markov transition matrix from previously discovered prime sequences
- Uses a 4-symbol alphabet: `{-1, 0, 1, 2}`
- Generates candidates with 75% probability following learned patterns, 25% random exploration
- Each new prime found improves future runs (cumulative learning)

### 2. Symbolic-First Pipeline (Tier 3)

```
Generate → Score → Residues → Partial Collapse → Full Collapse → Miller-Rabin
```

| Stage | Operation | Rejection |
|-------|-----------|-----------|
| ① | Symbolic score gate (O(n) pattern match) | ~50% |
| ② | Symbolic residue mod {3,5,7,11,13,17,19,23} | ~60% of remaining |
| ③ | Partial collapse — parity + mod 65537 | ~25% of remaining |
| ④ | Full GMP integer collapse | only ~12% of candidates reach here |
| ⑤ | Miller-Rabin (15 rounds, 2,3,5 + 12 random witnesses) | ~99.96% |

**Key result:** Only ~12% of candidates require an expensive big-integer construction. The rest are eliminated by cheap symbolic operations.

### 3. Historical Learning Database
- SQLite stores every discovered prime (timestamp, digit count, entropy, symbol sequence, PhaseToken hashes)
- Loaded automatically for targets ≥ 8,000 digits
- Pattern matrix trained on all historical sequences — each new discovery sharpens future guidance

---

## Configuration

Edit `src/main.rs` line 646:

```rust
let config = Config::new(16700);  // Target: ~10,053 digits
```

### Sequence Length Reference

| Target Digits | Sequence Length | Typical Time (16 cores) |
|---|---|---|
| ~2,466 | 4,096 | 20–40s |
| ~4,932 | 8,192 | 100–200s |
| ~8,000 | 13,300 | 8–15min |
| ~10,053 | 16,700 | 3–20min |

### Config Parameters (`src/main.rs`)

```rust
struct Config {
    sequence_len: usize,           // Symbol sequence length
    primality_rounds: u32,         // Miller-Rabin rounds: 15
    max_attempts: u64,             // Search limit: 300,000
    pattern_guide_ratio: f64,      // Learned pattern use: 75%
    num_threads: usize,            // All physical cores (auto-detect)
    symbolic_score_threshold: f64, // Tier 3 gate: 0.24
}
```

---

## Performance

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
- [ ] GPU acceleration for Miller-Rabin (CUDA port of `collapse_fast()`)
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
