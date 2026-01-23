# OpenPFGW Verification Instructions

## Quick Start

To verify our discovered primes with OpenPFGW:

### 1. Download OpenPFGW

```bash
# Linux
wget https://sourceforge.net/projects/openpfgw/files/pfgw-4.1.7_linux.7z/download -O pfgw.7z
7z x pfgw.7z
chmod +x pfgw64

# macOS/Windows: Download from https://sourceforge.net/projects/openpfgw/
```

### 2. Run Verification

```bash
# Test 2,467-digit prime (takes ~1-2 minutes)
./pfgw64 openpfgw_2467digits_input.txt -l"verify_2467.log"

# Test 4,931-digit prime (takes ~10-20 minutes)
./pfgw64 openpfgw_4931digits_input.txt -l"verify_4931.log"
```

### 3. Check Results

```bash
tail verify_2467.log
tail verify_4931.log
```

Look for:
- `PRIME` or `PRP` (Probable Prime) = Success ✅
- `COMPOSITE` = Not prime
- Certificate information = Submission ready

## Files Provided

- `openpfgw_2467digits_input.txt` - 2,467-digit prime in ABC format
- `openpfgw_4931digits_input.txt` - 4,931-digit prime in ABC format
- `quanjp_ultimate_2467digits.txt` - Full prime data with entropy/sequence info
- `quanjp_ultimate_4931digits.txt` - Full prime data with entropy/sequence info

## What OpenPFGW Does

OpenPFGW uses **George Woltman's gwnum library** (same as GIMPS) to perform:

1. **Fermat tests** - Initial filtering
2. **Miller-Rabin tests** - Strong probabilistic testing
3. **Deterministic tests** - Where mathematically possible
4. **Generation of certificates** - Proof of primality

The tool creates detailed test logs showing:
- FFT size used
- Test progress
- Residue values
- Final verdict

## Prime Pages Submission

Once verified:

1. Get the OpenPFGW certificate from the log file
2. Visit https://t5k.org/submit/
3. Submit:
   - Prime number (decimal)
   - Discovery method
   - Your name/contact
   - Certificate of testing
   - Reference to this project

## Our Discovered Primes

### 4,931-Digit Prime
- **Discovery time**: 197.20 seconds
- **Algorithm**: ML pattern-guided generation + Fermat (2 bases) + Miller-Rabin (15 rounds)
- **Confidence**: 99.9%+ (additional Fermat + strong M-R rounds)
- **Entropy**: 1.908072
- **Hardware**: 16-core processor (15 threads used)

### 2,467-Digit Prime
- **Discovery time**: 11.72 seconds
- **Algorithm**: ML pattern-guided generation + Fermat (2 bases) + Miller-Rabin (15 rounds)
- **Confidence**: 99.9%+ (additional Fermat + strong M-R rounds)
- **Entropy**: 1.906320
- **Hardware**: 16-core processor (15 threads used)

## Expected OpenPFGW Results

For numbers this size, OpenPFGW will take:
- **2,467 digits**: 1-5 minutes
- **4,931 digits**: 10-30 minutes

Performance depends on:
- CPU power
- FFT size selection
- Number of test rounds needed for confidence

## References

- **OpenPFGW Project**: https://sourceforge.net/projects/openpfgw/
- **Prime Pages Database**: https://t5k.org
- **GIMPS (uses same gwnum)**: https://www.mersenne.org/
- **Prime Verification Guide**: https://t5k.org/howto.html
