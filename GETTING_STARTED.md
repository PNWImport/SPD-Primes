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
1. Detects your CPUs and uses all physical cores
2. Loads training patterns from `data/prime_history.db` (if available)
3. Builds a symbolic seed of the target size (~20,000 digits by default)
4. Sieves the window `seed, seed+2, seed+4, …` and tests the survivors on all cores
5. Saves the result to `quanjp_prime_NN_XXXXXdigits.txt` in the current directory

**Expected output (10K-digit run, `quanjp-prime-hunter 16700`):**

```
🚀 QuanJP Prime Hunter 2050 - SYMBOLIC-FIRST EDITION
   ⚡ Symbolic seed + deep sieve + numeric verification

💻 System Detection:
   Physical CPUs:  16
   Logical CPUs:   16
   Using threads:  16 (all cores)

🧠 Loading & Learning Patterns...
   ✓ Loaded 4 historical sequences from database

⚙️  Configuration:
   Sequence length:        16700
   Target digits:          ~10054
   Primes to find:         1
   Miller-Rabin rounds:    15 (+ Baillie-PSW)
   Pattern guidance:       75%

🔍 Hunt 1/1 ...

✅ 🎉 PRIME #1 DISCOVERED!
   Digits:      10055
   Hunt time:   ...

📊 Pipeline Statistics:
   Sieve bound:                   ...
   Survived sieve:                ...
   Primality tests run:           ...

💾 Saved to quanjp_prime_01_10055digits.txt
```

---

## Changing Target Digit Size

Pass the sequence length (and optionally how many primes to find) on the command line.
No rebuild needed. Digits ≈ sequence length × 0.602.

```bash
./target/release/quanjp-prime-hunter 4096       # ~2,466 digits
./target/release/quanjp-prime-hunter 8192       # ~4,932 digits
./target/release/quanjp-prime-hunter 13300      # ~8,000 digits
./target/release/quanjp-prime-hunter 16700      # ~10,053 digits
./target/release/quanjp-prime-hunter 33200      # ~20,000 digits  ← default with no argument
./target/release/quanjp-prime-hunter 8192 10    # ten ~4,932-digit primes in one run
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

**"Window exhausted"**
- Extremely unlikely (the window holds ~30 expected primes); the hunter picks a new seed and continues on its own

**"Out of memory"**
- Reduce sequence length

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
