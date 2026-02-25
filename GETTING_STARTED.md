# Getting Started with QuanJP Prime Hunter

## Installation

### Prerequisites
- Rust 1.70+ ([install here](https://rustup.rs/))
- Linux/macOS/Windows with gcc/clang
- 16GB+ RAM recommended
- Multi-core CPU (16+ cores optimal)

### Clone & Build

```bash
git clone https://github.com/PNWImport/SPD-Primes.git
cd SPD-Primes

# Build optimized binary
cargo build --release

# Binary location
./target/release/quanjp-prime-hunter
```

---

## Quick Run

### Run with Default Configuration (4K target)

```bash
# Just run it!
./target/release/quanjp-prime-hunter
```

**What happens:**
1. Detects 16 CPUs, uses 15 threads
2. Loads training patterns (hardcoded defaults)
3. Starts searching for ~4,000 digit prime
4. Expected time: 50-100 seconds
5. Saves result to `quanjp_ultimate_XXXX_digits.txt`

### Expected Output

```
🚀 QuanJP Prime Hunter 2050 - ULTIMATE EDITION
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

💻 System Detection:
   Physical CPUs:  16
   Logical CPUs:   16
   Using threads:  15 (CPU - 1 for OS)

🧠 Loading & Learning Patterns...
   ✅ Learned from sequence 1 (16 symbols)
   ✅ Learned from sequence 2 (16 symbols)

⚙️  Configuration:
   Sequence length:        6644
   Target digits:          ~4000
   ...

🚀 Starting Prime Hunt...

✅ 🎉 PRIME DISCOVERED!
   Digits:      3999
   Entropy:     1.913272
   ...
   Duration:              52.73s
   Rate:                  63 attempts/sec

💾 Saved to quanjp_ultimate_3999digits.txt
```

---

## Changing Target Digit Size

Edit `src/main.rs` line 511:

```rust
let config = Config::new(6644);  // Target: ~4,000 digits
```

### Common Targets

```rust
// Small (fast)
Config::new(4096)    // ~2,466 digits, ~20-40s

// Medium (balanced)
Config::new(6644)    // ~4,000 digits, ~50-100s
Config::new(8192)    // ~4,932 digits, ~100-200s

// Large (slow, needs DB learning)
Config::new(13300)   // ~8,000 digits, 30+ minutes
Config::new(13609)   // ~8,193 digits, 20+ minutes
```

Then rebuild:

```bash
cargo build --release
./target/release/quanjp-prime-hunter
```

---

## Using Historical Database

### Ingest Previous Results

After running multiple searches, populate the database:

```bash
cargo build --release --bin ingest-history
./target/release/ingest-history
```

**Output:**
```
Processing: results/quanjp_ultimate_2466digits.txt
  ✓ Saved 2466-digit prime
Processing: results/quanjp_ultimate_4931digits.txt
  ✓ Saved 4931-digit prime
...
✅ Ingestion complete! Processed 8 discoveries
Database now contains 8 total discoveries:
  Digit sizes: 2466,2467,3999,4000,4095,4931,4933,8193
```

### How Database Helps

For targets ≥ 8,000 digits:
- Database loads historically successful sequences
- Uses proven patterns to guide candidate generation
- Slightly slower startup (~1-2 seconds overhead)
- Better pattern quality → fewer attempts needed

For targets < 8,000 digits:
- Database skipped automatically
- Uses fast hardcoded defaults
- No overhead, maximum speed

---

## Monitoring a Long Run

### Background Execution

```bash
# Run in background
nohup ./target/release/quanjp-prime-hunter > prime_hunt.log 2>&1 &

# Monitor progress
tail -f prime_hunt.log

# Check process
ps aux | grep quanjp
```

### Parallelization Details

**Current Configuration (src/main.rs:417-432):**
- Physical cores detected: 16
- Threads allocated: 15 (1 reserved for OS)
- Parallel search: Independent per thread
- Lock-free generation: No contention
- Atomic counters: Thread-safe statistics

**To adjust threads:**
Edit `Config::new()` in `src/main.rs`:
```rust
let num_threads = if physical_cpus > 1 {
    physical_cpus - 1  // Adjust this
} else {
    1
};
```

---

## Understanding Results

### Result File Format

```
QuanJP Ultimate Prime
====================
Digits: 3999
Entropy: 1.913272
Sequence Length: 6644

PhaseToken
==========
Timestamp: 1770366581054
Session Hash: 684aaaec5107a9ee5769dc3e19dc2ff5...
Result Hash: ef514f0273d84940518549a91307662c...

Prime:
[8,000+ digit number...]

Symbols:
[-1, 0, 1, 2, -1, 1, 0, ...] (6644 symbols)
```

**What each field means:**

- **Digits:** Confirmed decimal digit count of prime
- **Entropy:** Shannon entropy of symbolic sequence (1.88-2.0 range)
- **Sequence Length:** Number of symbols used (determines digit size)
- **PhaseToken:** Cryptographic proof of discovery
- **Prime:** The actual prime number (base 10)
- **Symbols:** The -1/0/1/2 sequence that generated it

### Verifying a Prime

With OpenPFGW (included):

```bash
cd tools
7z x pfgw64-4.1.7_linux.7z
cd ..

# Create input file
echo "ABC [base10:../results/quanjp_ultimate_3999digits.txt]" > verify.txt

# Run verification
./tools/pfgw64 -f verify.txt
```

---

## Troubleshooting

### "Compiled for release, not debug"
```bash
cargo build --release
# (not cargo build)
```

### "Out of memory"
- Reduce sequence length (fewer symbols = less memory)
- Or run on machine with more RAM
- Each search uses ~1-2GB

### "No prime found in X attempts"
- Target size too large for max_attempts limit
- Increase `max_attempts` in `Config::new()`
- Or reduce target digit size

### "Database missing"
```bash
./target/release/ingest-history  # Auto-creates and populates
```

---

## Performance Tips

1. **Use 15 cores on 16-core CPU** (reserve 1 for OS)
2. **Target 4K-6K digits** for reliable, fast results
3. **Run 8K+ overnight** (takes 20-30 minutes)
4. **Disable background services** when running large searches
5. **Use NVMe SSD** for SQLite database (if using DB)

---

## Next Steps

1. ✅ Run with default config
2. ✅ Try different target sizes
3. ✅ Build database from results
4. ✅ Experiment with entropy threshold
5. 📖 Read `README.md` for technical details
6. 📊 Check `DISCOVERIES.md` for performance insights

---

## Questions?

- 📖 See `docs/` folder for detailed guides
- 🔧 Edit `src/main.rs` to understand the algorithm
- 🗄️ Check `data/prime_history.db` via SQLite CLI
- 📝 Review past discoveries in `results/` folder

---

**Happy Prime Hunting!** 🚀
