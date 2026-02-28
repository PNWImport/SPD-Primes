# Getting Started with QuanJP Prime Hunter

## Prerequisites

- Rust 1.70+ ([install here](https://rustup.rs/))
- Linux with gcc/clang (GMP via `rug` crate)
- 8GB+ RAM (16GB+ recommended for 10K+ digit targets)
- Multi-core CPU (16 cores optimal)

## Clone & Build

```bash
git clone https://github.com/PNWImport/SPD-Primes.git
cd SPD-Primes

cargo build --release
```

Binaries:
- `./target/release/quanjp-prime-hunter` — prime discovery
- `./target/release/ingest-history` — populate the learning database

---

## Quick Run

```bash
./target/release/quanjp-prime-hunter
```

**What happens:**
1. Detects 16 CPUs, uses all 16 threads
2. Loads training patterns from `data/prime_history.db` (if available)
3. Searches for a ~10,053-digit prime (sequence length 16,700)
4. Saves result to `results/quanjp_prime_NN_XXXXXdigits.txt`

**Expected output:**

```
🚀 QuanJP Prime Hunter 2050 - SYMBOLIC-FIRST EDITION
   ⚡ Tier 3: Symbolic exploration + numeric verification

💻 System Detection:
   Physical CPUs:  16
   Logical CPUs:   16
   Using threads:  16 (all cores)

🧠 Loading & Learning Patterns...
   ✓ Loaded 4 historical sequences from database

⚙️  Configuration:
   Sequence length:        16700
   Target digits:          ~10054
   Symbolic score gate:    > 0.24
   Miller-Rabin rounds:    15
   Max attempts:           300000
   Pattern guidance:       75%

🔍 Hunt 1/10 ...

✅ 🎉 PRIME DISCOVERED!
   Digits:      10055
   Duration:    213.37s
   Attempts:    6,764

💾 Saved to results/quanjp_prime_01_10055digits.txt
```

---

## Changing Target Digit Size

Edit `src/main.rs` line 646:

```rust
let config = Config::new(16700);  // ~10,053 digits
```

### Common Targets

```rust
Config::new(4096)    // ~2,466 digits  — ~20–40s
Config::new(8192)    // ~4,932 digits  — ~100–200s
Config::new(13300)   // ~8,000 digits  — 8–15 min
Config::new(16700)   // ~10,053 digits — 3–20 min  ← default
```

Then rebuild and run:

```bash
cargo build --release
./target/release/quanjp-prime-hunter
```

---

## Building the Learning Database

The database lets each run learn from all previous discoveries.

```bash
cargo build --release --bin ingest-history
./target/release/ingest-history
```

**Output:**

```
Processing: results/quanjp_ultimate_9994digits.txt
  ✓ Saved 9994-digit prime
Processing: results/quanjp_prime_01_10055digits.txt
  ✓ Saved 10055-digit prime
...
✅ Ingestion complete! 28 total discoveries in database.
```

The database activates automatically for targets ≥ 8,000 digits. Below that the overhead
isn't worth it — hardcoded defaults are used instead.

---

## Running in the Background

```bash
# Run 10 hunts, log to file
nohup ./target/release/quanjp-prime-hunter > prime_hunt.log 2>&1 &

# Watch progress
tail -f prime_hunt.log

# Check process
pgrep -a quanjp
```

---

## Understanding the Result Files

```
QuanJP Ultimate Prime #01
====================
Digits: 10055
Entropy: 1.991835
Sequence Length: 16700

PhaseToken
==========
Timestamp: 1772305980122
Session Hash: f15579952dd83646...
Result Hash:  5dad8415119f2d78...

Prime:
190152113551574791893781...  (10,055 digits)

Symbols:
[-1, 0, 1, 2, -1, 1, 0, ...]  (16,700 symbols)
```

| Field | Meaning |
|-------|---------|
| Digits | Confirmed decimal digit count |
| Entropy | Shannon entropy of symbolic sequence (1.88–2.0 range) |
| Sequence Length | Number of symbols (controls target digit size) |
| PhaseToken | Cryptographic proof of discovery |
| Prime | The prime number in base 10 |
| Symbols | The -1/0/1/2 sequence that generated it |

---

## Verifying a Prime

```bash
# Extract pfgw
cd tools && 7z x pfgw64-4.1.7_linux.7z && cd ..

# Run verification
./tools/pfgw64 results/quanjp_prime_01_10055digits.txt
```

See `docs/OPENPFGW_INSTRUCTIONS.md` for the full workflow.

---

## Troubleshooting

**"No prime found in X attempts"**
- Increase `max_attempts` in `Config::new()` in `src/main.rs`
- Or reduce target size

**"Out of memory"**
- Reduce sequence length
- Each 10K-digit search uses ~2–4GB RAM

**"Database missing"**
```bash
./target/release/ingest-history   # creates and populates data/prime_history.db
```

**"Compiled without optimizations (slow)"**
```bash
cargo build --release   # always use --release
```

---

## Performance Tips

1. **Let it run all 16 cores** — current default, no action needed
2. **Build the database first** — dramatically cuts attempts for 8K+ targets
3. **Run overnight for 10K hunts** — typical time 3–20 min per prime
4. **Disable heavy background services** during a run
5. **NVMe SSD** helps for SQLite DB I/O (small effect overall)

---

## File Organization

All discovered primes live in `results/`:

```
results/
├── quanjp_prime_NN_XXXXXdigits.txt   ← batch-run format (current)
└── quanjp_ultimate_XXXXdigits.txt    ← legacy single-run format
```

See `DISCOVERIES.md` for the full discovery log with timing data.

---

**Happy Prime Hunting!**
