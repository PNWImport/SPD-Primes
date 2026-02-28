/// Batch Statistical Experiment for Prime Density Analysis
///
/// Tests whether the QuanJP symbolic pipeline finds primes faster
/// than expected by the Prime Number Theorem.
///
/// Methodology (from the "separate emotion from math" review):
///   H0: mean attempts ≈ ln(N)  → no density bias, pure engineering
///   H1: mean attempts < ln(N)  → structural bias toward primes exists
///
/// Usage:
///   batch-stats [--trials N] [--sequence-len L] [--max-attempts M]
///
/// Defaults: --trials 50 --sequence-len 4096 --max-attempts 60000

use rand::prelude::*;
use rayon::prelude::*;
use num_bigint::{BigUint, RandBigInt};
use num_traits::{Zero, One};
use std::sync::atomic::{AtomicU64, Ordering};
use lazy_static::lazy_static;
use std::cell::RefCell;
use std::time::Instant;

// ============================================================================
// THREAD-LOCAL RNG
// ============================================================================

thread_local! {
    static THREAD_RNG: RefCell<ThreadRng> = RefCell::new(thread_rng());
}

// ============================================================================
// PATTERN MATRIX
// ============================================================================

#[derive(Clone)]
struct PatternMatrix {
    transitions: [[f64; 4]; 4],
}

impl PatternMatrix {
    fn uniform() -> Self {
        Self { transitions: [[0.25; 4]; 4] }
    }

    fn learn_from_sequence(&mut self, symbols: &[i8]) {
        let mut counts = [[0usize; 4]; 4];
        for i in 0..symbols.len().saturating_sub(1) {
            let a = (symbols[i] + 1) as usize;
            let b = (symbols[i + 1] + 1) as usize;
            if a < 4 && b < 4 {
                counts[a][b] += 1;
            }
        }
        for i in 0..4 {
            let row_sum: usize = counts[i].iter().sum();
            if row_sum > 0 {
                for j in 0..4 {
                    let new_prob = counts[i][j] as f64 / row_sum as f64;
                    self.transitions[i][j] = 0.7 * new_prob + 0.3 * self.transitions[i][j];
                }
            }
        }
    }

    #[inline]
    fn predict_next(&self, prev: i8, rng: &mut ThreadRng) -> i8 {
        let idx = (prev + 1) as usize;
        if idx >= 4 { return *[-1i8, 0, 1, 2].choose(rng).unwrap(); }
        let probs = &self.transitions[idx];
        let r: f64 = rng.gen();
        let mut cum = 0.0;
        for (i, &p) in probs.iter().enumerate() {
            cum += p;
            if r < cum { return (i as i8) - 1; }
        }
        2
    }
}

// ============================================================================
// POWER CACHE
// ============================================================================

lazy_static! {
    static ref POWER_CACHE: Vec<BigUint> = {
        let mut cache = Vec::with_capacity(20000);
        let four = BigUint::from(4u32);
        let mut power = BigUint::one();
        for _ in 0..20000 {
            cache.push(power.clone());
            power *= &four;
        }
        cache
    };
}

// ============================================================================
// PIPELINE STAGES (identical logic to main.rs)
// ============================================================================

#[inline]
fn generate_symbolic_guided(length: usize, pattern: &PatternMatrix, guide_ratio: f64) -> Vec<i8> {
    let mut symbols = Vec::with_capacity(length);
    THREAD_RNG.with(|rng_cell| {
        let mut rng = rng_cell.borrow_mut();
        symbols.push(*[-1i8, 0, 1, 2].choose(&mut *rng).unwrap());
        for _ in 1..length {
            if rng.gen::<f64>() < guide_ratio {
                let prev = *symbols.last().unwrap();
                symbols.push(pattern.predict_next(prev, &mut *rng));
            } else {
                symbols.push(*[-1i8, 0, 1, 2].choose(&mut *rng).unwrap());
            }
        }
    });
    symbols
}

#[inline]
fn symbolic_score(symbols: &[i8], pattern: &PatternMatrix) -> f64 {
    let mut score = 0.0;
    for i in 1..symbols.len() {
        let a = (symbols[i-1] + 1) as usize;
        let b = (symbols[i] + 1) as usize;
        if a < 4 && b < 4 { score += pattern.transitions[a][b]; }
    }
    score / symbols.len() as f64
}

/// Oddness gate: checks if the collapsed number would be odd.
/// N = Σ val_i * 4^i. Since 4^i is even for i >= 1, parity depends only on val_0.
/// val_0 is odd (1 or 3) when symbols[0] ∈ {-1, 1}. No prime > 2 is even.
/// This single check eliminates ~50% of candidates before any expensive work.
#[inline]
fn is_symbolically_odd(symbols: &[i8]) -> bool {
    // symbols[0]: -1 → val 3 (odd), 0 → val 0 (even), 1 → val 1 (odd), 2 → val 2 (even)
    symbols[0] == -1 || symbols[0] == 1
}

const RESIDUE_MODS: [u32; 8] = [3, 5, 7, 11, 13, 17, 19, 23];

#[inline]
fn symbolic_residues(symbols: &[i8]) -> [u32; 8] {
    let mut res = [0u32; 8];
    let mut pow = [1u32; 8];
    for &s in symbols {
        let val = if s == -1 { 3u32 } else { s as u32 };
        for i in 0..8 {
            res[i] = (res[i] + val * pow[i]) % RESIDUE_MODS[i];
            pow[i] = (pow[i] * 4) % RESIDUE_MODS[i];
        }
    }
    res
}

#[inline]
fn entropy(symbols: &[i8]) -> f64 {
    let mut counts = [0usize; 4];
    for &s in symbols { counts[(s + 1) as usize] += 1; }
    let total = symbols.len() as f64;
    let mut h = 0.0;
    for &c in &counts {
        if c > 0 { let p = c as f64 / total; h -= p * p.log2(); }
    }
    h
}

#[inline]
fn partial_collapse_check(symbols: &[i8]) -> bool {
    let check_len = symbols.len().min(256);
    let mut parity = 0u32;
    let mut acc = 0u32;
    let mut pow = 1u32;
    for &s in &symbols[..check_len] {
        let val = if s == -1 { 3u32 } else { s as u32 };
        parity ^= val;
        acc = (acc + val * pow) % 65537;
        pow = (pow * 4) % 65537;
    }
    acc != 0 && parity > 0 && parity < 256
}

#[inline]
fn collapse_fast(symbols: &[i8]) -> BigUint {
    let mut result = BigUint::zero();
    for (i, &s) in symbols.iter().enumerate() {
        let val = if s == -1 { 3u32 } else { s as u32 };
        if val > 0 { result += BigUint::from(val) * &POWER_CACHE[i]; }
    }
    result
}

/// Miller-Rabin with deterministic small bases first.
/// Bases 2 and 3 catch most composites immediately — no RNG overhead,
/// no BigUint random generation. Remaining rounds use random witnesses.
#[inline]
fn is_prime_miller_rabin(n: &BigUint, rounds: u32) -> bool {
    if n < &BigUint::from(2u32) { return false; }
    if n == &BigUint::from(2u32) || n == &BigUint::from(3u32) { return true; }
    if !n.bit(0) { return false; }
    let n_minus_1 = n - BigUint::one();
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while !d.bit(0) { d >>= 1; r += 1; }

    // Phase 1: deterministic bases (no RNG cost, catches most composites)
    let det_bases: [u32; 3] = [2, 3, 5];
    let det_count = det_bases.len().min(rounds as usize);
    for &base in &det_bases[..det_count] {
        let a = BigUint::from(base);
        if &a >= &n_minus_1 { continue; }
        let mut x = a.modpow(&d, n);
        if x == BigUint::one() || x == n_minus_1 { continue; }
        let mut composite = true;
        for _ in 0..r.saturating_sub(1) {
            x = x.modpow(&BigUint::from(2u32), n);
            if x == n_minus_1 { composite = false; break; }
        }
        if composite { return false; }
    }

    // Phase 2: random witnesses for remaining rounds
    let random_rounds = rounds.saturating_sub(det_count as u32);
    if random_rounds == 0 { return true; }
    THREAD_RNG.with(|rng_cell| {
        let mut rng = rng_cell.borrow_mut();
        for _ in 0..random_rounds {
            let a = rng.gen_biguint_range(&BigUint::from(2u32), &n_minus_1);
            let mut x = a.modpow(&d, n);
            if x == BigUint::one() || x == n_minus_1 { continue; }
            let mut composite = true;
            for _ in 0..r.saturating_sub(1) {
                x = x.modpow(&BigUint::from(2u32), n);
                if x == n_minus_1 { composite = false; break; }
            }
            if composite { return false; }
        }
        true
    })
}

// ============================================================================
// TRIAL RUNNER
// Returns the number of attempts made before finding a prime, or None if
// max_attempts was reached without finding one.
// ============================================================================

fn run_single_trial(
    sequence_len: usize,
    max_attempts: u64,
    pattern: &PatternMatrix,
    mr_rounds: u32,
) -> Option<u64> {
    let attempts = AtomicU64::new(0);
    let score_threshold = 0.24f64;
    let entropy_threshold = 1.88f64;

    let result = (0..max_attempts).into_par_iter().find_map_any(|_| {
        attempts.fetch_add(1, Ordering::Relaxed);

        let symbols = generate_symbolic_guided(sequence_len, pattern, 0.75);

        // GATE 0: Oddness — costs 1 comparison, eliminates ~50% before anything else
        if !is_symbolically_odd(&symbols) { return None; }

        let score = symbolic_score(&symbols, pattern);
        if score < score_threshold { return None; }

        // Expanded residues: mod {3,5,7,11,13,17,19,23} — 8 primes, all u32
        let residues = symbolic_residues(&symbols);
        for &r in &residues {
            if r == 0 { return None; }
        }

        let ent = entropy(&symbols);
        if ent < entropy_threshold { return None; }

        if !partial_collapse_check(&symbols) { return None; }

        let n = collapse_fast(&symbols);
        if is_prime_miller_rabin(&n, mr_rounds) { Some(()) } else { None }
    });

    if result.is_some() {
        Some(attempts.load(Ordering::Relaxed))
    } else {
        None
    }
}

// ============================================================================
// STATISTICS
// ============================================================================

/// Theoretical expected attempts for a number of `digits` decimal digits.
/// From PNT: E[attempts] ≈ ln(10^d) = d * ln(10)
fn theoretical_expected(digits: usize) -> f64 {
    digits as f64 * 2.302585_f64
}

/// One-sample t-test.
/// Returns (t_statistic, standard_error).
/// H0: population mean = null_mean  (no density bias)
/// H1: population mean < null_mean  (system finds primes faster than random)
fn one_sample_t_test(data: &[u64], null_mean: f64) -> (f64, f64) {
    let n = data.len() as f64;
    let mean: f64 = data.iter().map(|&x| x as f64).sum::<f64>() / n;
    let variance: f64 = data.iter()
        .map(|&x| { let diff = x as f64 - mean; diff * diff })
        .sum::<f64>() / (n - 1.0);
    let std_err = (variance / n).sqrt();
    let t = (mean - null_mean) / std_err;
    (t, std_err)
}

fn std_dev(data: &[u64]) -> f64 {
    let n = data.len() as f64;
    let mean: f64 = data.iter().map(|&x| x as f64).sum::<f64>() / n;
    let variance: f64 = data.iter()
        .map(|&x| { let diff = x as f64 - mean; diff * diff })
        .sum::<f64>() / (n - 1.0);
    variance.sqrt()
}

fn median(data: &mut Vec<u64>) -> f64 {
    data.sort_unstable();
    let n = data.len();
    if n % 2 == 0 {
        (data[n/2 - 1] + data[n/2]) as f64 / 2.0
    } else {
        data[n/2] as f64
    }
}

/// Approximate normal CDF using Abramowitz and Stegun 26.2.17.
/// Good to ~5 decimal places. Used for one-tailed p-value approximation.
fn normal_cdf(x: f64) -> f64 {
    let b1 =  0.319381530_f64;
    let b2 = -0.356563782_f64;
    let b3 =  1.781477937_f64;
    let b4 = -1.821255978_f64;
    let b5 =  1.330274429_f64;
    let p  =  0.2316419_f64;
    let t = 1.0 / (1.0 + p * x.abs());
    let poly = t*(b1 + t*(b2 + t*(b3 + t*(b4 + t*b5))));
    let phi = (-x*x/2.0).exp() / (2.0 * std::f64::consts::PI).sqrt();
    if x >= 0.0 { 1.0 - phi * poly } else { phi * poly }
}

// ============================================================================
// CLI ARG PARSING
// ============================================================================

struct Args {
    trials: usize,
    sequence_len: usize,
    max_attempts: u64,
    threads: usize,
    mr_rounds: u32,
}

fn parse_args() -> Args {
    let args: Vec<String> = std::env::args().collect();
    let mut trials = 50usize;
    let mut sequence_len = 4096usize;
    let mut max_attempts = 60_000u64;
    let mut threads = num_cpus::get_physical().saturating_sub(1).max(1);
    let mut mr_rounds = 15u32;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--trials"       => { i += 1; if i < args.len() { trials = args[i].parse().unwrap_or(trials); } }
            "--sequence-len" => { i += 1; if i < args.len() { sequence_len = args[i].parse().unwrap_or(sequence_len); } }
            "--max-attempts" => { i += 1; if i < args.len() { max_attempts = args[i].parse().unwrap_or(max_attempts); } }
            "--threads"      => { i += 1; if i < args.len() { threads = args[i].parse().unwrap_or(threads); } }
            "--mr-rounds"    => { i += 1; if i < args.len() { mr_rounds = args[i].parse().unwrap_or(mr_rounds); } }
            "--help" | "-h" => {
                println!("Usage: batch-stats [OPTIONS]\n");
                println!("  --trials N         Number of independent trials (default: 50)");
                println!("  --sequence-len L   Symbol sequence length (default: 4096 → ~2466 digits)");
                println!("  --max-attempts M   Give up on a trial after M attempts (default: 60000)");
                println!("  --threads T        Rayon threads per trial (default: physical_cpus - 1)");
                println!("  --mr-rounds R      Miller-Rabin rounds (default: 15, min 3)");
                println!("                     First 3 use deterministic bases {{2,3,5}}; rest random.");
                println!("                     P(false positive) <= 4^(-R). 15 → ~10^-9.");
                std::process::exit(0);
            }
            _ => {}
        }
        i += 1;
    }

    if mr_rounds < 3 { mr_rounds = 3; }

    Args { trials, sequence_len, max_attempts, threads, mr_rounds }
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    let args = parse_args();
    let target_digits = (args.sequence_len as f64 * 0.602).ceil() as usize;
    let theoretical = theoretical_expected(target_digits);

    rayon::ThreadPoolBuilder::new()
        .num_threads(args.threads)
        .build_global()
        .unwrap();

    println!("\n═══════════════════════════════════════════════════════════════════════");
    println!("  BATCH PRIME DENSITY EXPERIMENT — Separate Emotion from Math");
    println!("═══════════════════════════════════════════════════════════════════════");
    println!("  Trials planned   : {}", args.trials);
    println!("  Sequence length  : {} symbols → ~{} digits", args.sequence_len, target_digits);
    println!("  Max per trial    : {} attempts", args.max_attempts);
    println!("  Rayon threads    : {}", args.threads);
    println!("  M-R rounds       : {} (first 3 deterministic: bases 2,3,5)", args.mr_rounds);
    println!("  Residue sieve    : mod {{3,5,7,11,13,17,19,23}} + oddness gate");
    println!();
    println!("  Null hypothesis  : mean attempts = ln(N) ≈ {:.0}", theoretical);
    println!("  Alt hypothesis   : mean attempts < {:.0}  (bias toward primes)", theoretical);
    println!();

    // Build a default pattern matrix from hardcoded fallback sequences
    // (same logic as main.rs load_training_data)
    let mut pattern = PatternMatrix::uniform();
    let seed_sequences: [&[i8]; 2] = [
        &[-1, -1, -1, -1, -1, -1, -1, 1, 0, -1, 2, 2, 1, -1, 2, 0],
        &[-1,  1,  0, -1,  0,  0,  0, 1, 1, -1, 1, -1, 1, 1, 1, -1],
    ];
    for seq in &seed_sequences {
        pattern.learn_from_sequence(seq);
    }
    // Also attempt to load saved JSON sequences
    for path in &["prime_1233_symbols.json", "prime_2466_symbols.json"] {
        if let Ok(data) = std::fs::read_to_string(path) {
            if let Ok(symbols) = serde_json::from_str::<Vec<i8>>(&data) {
                pattern.learn_from_sequence(&symbols);
            }
        }
    }

    let mut successes: Vec<u64> = Vec::with_capacity(args.trials);
    let mut failures = 0usize;
    let total_start = Instant::now();

    println!("  Trial │ Attempts │ Status");
    println!("  ──────┼──────────┼───────────");

    for trial in 1..=args.trials {
        let trial_start = Instant::now();
        let outcome = run_single_trial(args.sequence_len, args.max_attempts, &pattern, args.mr_rounds);
        let elapsed = trial_start.elapsed();

        match outcome {
            Some(attempts) => {
                println!("   {:4} │ {:8} │ FOUND  ({:.1}s)",
                    trial, attempts, elapsed.as_secs_f64());
                successes.push(attempts);
            }
            None => {
                println!("   {:4} │ {:>8} │ TIMEOUT (>{} attempts, {:.1}s)",
                    trial, "—", args.max_attempts, elapsed.as_secs_f64());
                failures += 1;
            }
        }
    }

    let total_elapsed = total_start.elapsed();

    println!();
    println!("═══════════════════════════════════════════════════════════════════════");
    println!("  RESULTS");
    println!("═══════════════════════════════════════════════════════════════════════");
    println!("  Total wall time  : {:.1}s", total_elapsed.as_secs_f64());
    println!("  Successful trials: {} / {}", successes.len(), args.trials);
    println!("  Timed-out trials : {}", failures);

    if successes.len() < 2 {
        println!("\n  Not enough successful trials for statistics. Increase --max-attempts.");
        return;
    }

    let n = successes.len();
    let mean: f64 = successes.iter().map(|&x| x as f64).sum::<f64>() / n as f64;
    let sd = std_dev(&successes);
    let mut sorted = successes.clone();
    let med = median(&mut sorted);
    let min_val = *sorted.first().unwrap();
    let max_val = *sorted.last().unwrap();

    println!();
    println!("  ── Descriptive Statistics ({} trials) ──", n);
    println!("  Mean attempts    : {:.1}", mean);
    println!("  Std deviation    : {:.1}", sd);
    println!("  Median           : {:.1}", med);
    println!("  Min              : {}", min_val);
    println!("  Max              : {}", max_val);
    println!("  Coeff. variation : {:.1}%  (geometric distribution predicts ~100%)", sd / mean * 100.0);

    println!();
    println!("  ── Comparison to Prime Number Theorem ──");
    println!("  Theoretical E[A] : {:.1}  (ln(10^{}) = {} × ln(10))", theoretical, target_digits, target_digits);
    println!("  Observed mean    : {:.1}", mean);
    println!("  Ratio obs/theory : {:.3}  (1.000 = no bias)", mean / theoretical);
    let pct_diff = (mean - theoretical) / theoretical * 100.0;
    if pct_diff < 0.0 {
        println!("  Relative diff    : {:.1}% FASTER than random expectation", -pct_diff);
    } else {
        println!("  Relative diff    : {:.1}% SLOWER than random expectation", pct_diff);
    }

    println!();
    println!("  ── One-Sample T-Test ──");
    println!("  H0: population mean = {:.1}  (no prime bias)", theoretical);
    println!("  H1: population mean < {:.1}  (faster than PNT)", theoretical);

    let (t_stat, se) = one_sample_t_test(&successes, theoretical);
    let _df = (n - 1) as f64;
    // For large df, t ≈ z (normal approximation)
    let p_value_one_tail = normal_cdf(t_stat);  // P(Z < t) = one-tailed p for H1: mean < μ0

    println!("  Standard error   : {:.1}", se);
    println!("  t-statistic      : {:.4}", t_stat);
    println!("  Degrees freedom  : {}", n - 1);
    println!("  p-value (1-tail) : {:.4}  (H1: mean < theoretical)", p_value_one_tail);

    println!();
    println!("  ── Interpretation ──");
    if p_value_one_tail < 0.01 {
        println!("  *** SIGNIFICANT (p < 0.01): The system finds primes significantly");
        println!("      faster than PNT predicts. Structural bias may exist.");
        println!("      Warrants further investigation.");
    } else if p_value_one_tail < 0.05 {
        println!("  *   MARGINAL (0.01 < p < 0.05): Slight tendency toward faster");
        println!("      discovery. Not conclusive. More trials recommended.");
    } else if p_value_one_tail > 0.95 {
        println!("  ✗   The system finds primes SLOWER than PNT predicts (p > 0.95).");
        println!("      Pipeline overhead may cause some prime candidates to be filtered.");
    } else {
        println!("  ✓   NO SIGNIFICANT BIAS (p = {:.4}).", p_value_one_tail);
        println!("      Observed mean is consistent with random prime density.");
        println!("      Conclusion: optimized prime hunter, not prime attractor.");
    }

    println!();
    println!("  ── Geometric Distribution Check ──");
    // For a geometric distribution, E[X] = μ and Var[X] ≈ μ² (for small p)
    // So we expect CV ≈ 100%. If observed CV is much less, distribution is non-geometric.
    let theoretical_cv = 100.0; // ~100% for geometric
    let observed_cv = sd / mean * 100.0;
    println!("  Theoretical CV   : ~{:.0}%  (geometric dist. with small p)", theoretical_cv);
    println!("  Observed CV      : {:.1}%", observed_cv);
    if (observed_cv - theoretical_cv).abs() < 20.0 {
        println!("  Distribution     : Consistent with geometric (expected for PNT density).");
    } else {
        println!("  Distribution     : DEVIATES from geometric. Investigate further.");
    }

    println!();
    println!("  ── Raw Data ──");
    println!("  attempts: {:?}", successes);
    println!("═══════════════════════════════════════════════════════════════════════");
}
