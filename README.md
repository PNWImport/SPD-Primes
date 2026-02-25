# QuanJP Prime Hunter 2050 - ULTIMATE EDITION

**Machine Learning-Guided Prime Number Discovery using Rust & Rayon**

A high-performance Rust application for discovering large probable primes (thousands to tens of thousands of digits) using ML-guided symbolic generation, multi-stage primality testing, and SQLite-backed historical learning.

---

## 🎯 Quick Start

```bash
# Build with optimizations
cargo build --release

# Run prime hunter (configurable target in src/main.rs:511)
./target/release/quanjp-prime-hunter

# Ingest historical results into database
./target/release/ingest-history
```

---

## 📊 Recent Discoveries

| Digits | Time | Attempts | Rate | Config | Status |
|--------|------|----------|------|--------|--------|
| **8,193** | 1,394.42s | 7,987 | 6/sec | 13609-seq | ✅ Found |
| **4,000** | 52.73s | 2,455 | 63/sec | 6644-seq (15 cores) | ✅ Found |
| **3,999** | 122.98s | 8,371 | 68/sec | 6644-seq (14 cores) | ✅ Found |
| **4,933** | 103.69s | 4,013 | 39/sec | 8192-seq | ✅ Found |
| **4,931** | 197.20s | 7,826 | 40/sec | 8192-seq | ✅ Found |
| **2,467** | 11.72s | 3,370 | 288/sec | 4096-seq | ✅ Found |
| **2,466** | 39.04s | 12,522 | 321/sec | 4096-seq | ✅ Found |

---

## 🏗️ Project Structure

```
SPD-Primes/
├── src/
│   ├── main.rs              # Main prime hunter algorithm
│   └── bin/
│       └── ingest_history.rs   # Historical data ingestion utility
├── docs/                    # Documentation & guides
├── results/                 # Discovered prime numbers
├── verification/            # OpenPFGW verification files
├── tools/                   # External utilities (pfgw binary)
├── data/                    # SQLite database
├── Cargo.toml              # Rust dependencies & config
└── README.md               # This file
```

---

## 🧠 How It Works

### 1. **ML-Guided Symbolic Generation**
- Learns pattern matrix from known prime sequences
- Uses 4-symbol alphabet: {-1, 0, 1, 2}
- Generates candidates with 75% probability following learned patterns
- 25% purely random for exploration

### 2. **Multi-Stage Primality Testing**

**Stage 1: Entropy Filter** (~100% pass)
- Symbol entropy must exceed 1.88/2.0
- Ensures candidate diversity

**Stage 2: Quick Composite Filter** (~80% elimination)
- Divisibility tests: 2, 3, 5, 7, 11, 13
- Instant rejection of obvious composites

**Stage 3: Fermat Test** (~99% elimination)
- Tests bases 2 and 3
- Fermat's Little Theorem: a^(p-1) ≡ 1 (mod p)

**Stage 4: Miller-Rabin Test** (final verdict)
- 15 probabilistic rounds
- Error rate: 2^-30 ≈ 99.9999% confidence
- Determines primality

### 3. **Historical Pattern Learning** (Optional for 8K+)
- SQLite database tracks all discoveries
- Lazy-loads patterns only for targets ≥ 8,000 digits
- Avoids overhead on smaller searches
- Improves candidate guidance for large primes

---

## ⚙️ Configuration

Edit `src/main.rs` line 511 to change target digit size:

```rust
let config = Config::new(6644);  // Target: ~4,000 digits
```

### Target Digit Reference Table

| Target Digits | Sequence Length | Expected Time (15 cores) |
|---|---|---|
| ~2,466 | 4,096 | ~30-40s |
| ~4,000 | 6,644 | ~50-100s |
| ~4,932 | 8,192 | ~100-200s |
| ~8,000 | 13,300 | ~30min+ |

### Key Config Parameters (src/main.rs:400-432)

```rust
struct Config {
    sequence_len: usize,           // Symbol sequence length
    entropy_threshold: f64,         // Min entropy: 1.88
    primality_rounds: u32,          // Miller-Rabin: 15
    max_attempts: u64,              // Search limit: 300,000
    pattern_guide_ratio: f64,       // Learned pattern use: 75%
    num_threads: usize,             // Auto-detect (CPU count - 1)
}
```

---

## 🔥 Performance Optimizations

### Core Scaling Results (4K target)
| Cores | Reservation | Duration | Winner |
|-------|-------------|----------|--------|
| **15** | 1 core | **52.73s** | ✅ **OPTIMAL** |
| 14 | 2 cores | 122.98s | |
| 16 | None | 187.47s | ❌ Contention |

**Finding:** Reserving 1 core for OS is optimal. Full CPU allocation causes thread scheduling contention.

### Database Integration
- **Lazy loading:** Only initialize DB for targets ≥ 8K
- **Smart pattern loading:** Load only sequences ±40% of target size
- **Zero overhead for small targets:** Fast hardcoded defaults for 4-6K

### Parallelization Strategy
- **Rayon thread pool:** Distributes candidates across CPU cores
- **Lock-free generation:** Each thread generates independently
- **Atomic statistics:** Counters for pipeline stages
- **Progress tracking:** Real-time attempt counter display

---

## 📚 Historical Database

### Build Database from Results

```bash
cargo build --release --bin ingest-history
./target/release/ingest-history
```

**Features:**
- Ingests all result files from `results/` folder
- Handles both old (no PhaseToken) and new (with PhaseToken) formats
- Stores: timestamp, digit count, entropy, sequence, hashes
- Supports legacy file formats with automatic timestamp generation

### Database Schema

```sql
CREATE TABLE discoveries (
    id INTEGER PRIMARY KEY,
    timestamp INTEGER NOT NULL,
    digits INTEGER NOT NULL,
    entropy REAL NOT NULL,
    sequence_length INTEGER NOT NULL,
    symbols TEXT NOT NULL,        -- JSON array
    session_hash TEXT NOT NULL,
    result_hash TEXT NOT NULL
);
```

---

## 🔐 PhaseToken - Cryptographic Proof

Each discovery includes a cryptographic PhaseToken:

```
PhaseToken
==========
Timestamp: 1770366581054
Session Hash: 684aaaec5107a9ee5769dc3e19dc2ff5...
Result Hash: ef514f0273d84940518549a91307662c...
```

**Components:**
- **Timestamp:** UTC milliseconds of discovery
- **Session Hash:** SHA3-512 of session ID + timestamp
- **Result Hash:** SHA3-512 of the prime number

This enables:
- Reproducible discovery verification
- Tamper-evident proof of generation
- Audit trail for scientific validation

---

## 🛠️ Development

### Build Release Binary
```bash
cargo build --release
```

### Run Tests (if added)
```bash
cargo test --release
```

### Profile Performance
```bash
time ./target/release/quanjp-prime-hunter
```

### Dependencies
- **rand** - Random number generation
- **rayon** - Data parallelization
- **num-bigint** - Arbitrary precision integers
- **rusqlite** - SQLite database (bundled)
- **indicatif** - Progress bars
- **colored** - Terminal colors
- **serde/serde_json** - Serialization
- **sha3** - Cryptographic hashing
- **chrono** - Timestamp handling

---

## 📖 Understanding Primality Testing

### Miller-Rabin Test (Probabilistic)
- Tests if number n is probably prime
- Each round: 2^-2 = 75% accuracy per round
- 15 rounds: (0.75)^15 ≈ 99.9999% confidence
- Much faster than deterministic tests on large numbers

### Why Probable Prime?
- True proof requires sophisticated algorithms (ECPP, AKS)
- Probable primes sufficient for cryptographic/research use
- T5K Prime Pages accept Miller-Rabin with proper documentation

---

## 🔗 Related Resources

- **Prime Pages (T5K):** https://primes.utm.edu/top20/ - Prime number records
- **OpenPFGW:** Prime verification tool (included in `tools/`)
- **Miller-Rabin:** https://en.wikipedia.org/wiki/Miller%E2%80%93Rabin_primality_test

---

## 📝 License

[Project license TBD]

---

## 🚀 Future Work

- [ ] ECPP verification for proven primes
- [ ] Larger digit targets (16K+)
- [ ] Optimized entropy threshold tuning
- [ ] GPU acceleration for Miller-Rabin
- [ ] Web interface for monitoring
- [ ] Distributed search across machines

---

**Last Updated:** February 2025
**Current Status:** Active Development
**Best Configuration:** 15 cores, 4K-6K digit targets for speed
