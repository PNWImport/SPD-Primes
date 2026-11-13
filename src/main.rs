use rand::prelude::*;
use rayon::prelude::*;
use rug::{Integer, integer::{IsPrime, Order}, rand::RandState};
use std::time::Instant;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use indicatif::{ProgressBar, ProgressStyle};
use colored::*;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_512};
use chrono::Utc;
use rusqlite::{Connection, params};
use std::cell::RefCell;

// ============================================================================
// THREAD-LOCAL RNG (Optimization: avoid RNG allocation overhead)
// ============================================================================

thread_local! {
    static THREAD_RNG: RefCell<ThreadRng> = RefCell::new(thread_rng());

    // Per-thread GMP RandState for rug witness generation — seeded once per thread
    static MR_RAND: RefCell<RandState<'static>> = {
        let mut rs = RandState::new();
        let seed_u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0xdeadbeef);
        rs.seed(&Integer::from(seed_u64));
        RefCell::new(rs)
    };
}

// ============================================================================
// DATABASE - Historical pattern and discovery tracking
// ============================================================================

fn init_database() -> rusqlite::Result<Connection> {
    std::fs::create_dir_all("data").ok();
    let conn = Connection::open("data/prime_history.db")?;

    conn.execute(
        "CREATE TABLE IF NOT EXISTS discoveries (
            id INTEGER PRIMARY KEY,
            timestamp INTEGER NOT NULL,
            digits INTEGER NOT NULL,
            entropy REAL NOT NULL,
            sequence_length INTEGER NOT NULL,
            symbols TEXT NOT NULL,
            session_hash TEXT NOT NULL,
            result_hash TEXT NOT NULL
        )",
        [],
    )?;

    Ok(conn)
}

fn save_discovery(
    conn: &Connection,
    timestamp: i64,
    digits: usize,
    entropy: f64,
    sequence_len: usize,
    symbols: &[i8],
    session_hash: &str,
    result_hash: &str,
) -> rusqlite::Result<()> {
    let symbols_json = serde_json::to_string(symbols).unwrap_or_default();

    conn.execute(
        "INSERT INTO discoveries (timestamp, digits, entropy, sequence_length, symbols, session_hash, result_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![timestamp, digits as i32, entropy, sequence_len as i32, symbols_json, session_hash, result_hash],
    )?;

    Ok(())
}

fn load_from_history(conn: &Connection, target_digits: usize) -> Vec<Vec<i8>> {
    let mut training_data = Vec::new();

    // Only load patterns within ±40% of target size to avoid overhead
    let min_digits = (target_digits as f64 * 0.6) as i32;
    let max_digits = (target_digits as f64 * 1.4) as i32;

    // Try to load the most recent discovery within acceptable range
    if let Ok(mut stmt) = conn.prepare(
        "SELECT symbols FROM discoveries
         WHERE digits BETWEEN ? AND ?
         ORDER BY ABS(digits - ?) ASC, timestamp DESC
         LIMIT 2"
    ) {
        if let Ok(rows) = stmt.query_map(params![min_digits, max_digits, target_digits as i32], |row| {
            row.get::<_, String>(0)
        }) {
            for row in rows {
                if let Ok(symbols_json) = row {
                    if let Ok(symbols) = serde_json::from_str::<Vec<i8>>(&symbols_json) {
                        training_data.push(symbols);
                    }
                }
            }
        }
    }

    training_data
}

// ============================================================================
// PHASETOKEN - Cryptographic proof of search session integrity
// ============================================================================

#[derive(Debug)]
struct PhaseToken {
    timestamp: i64,
    session_hash: String,
    result_hash: String,
}

impl PhaseToken {
    fn new(session_id: &str, result: &Integer) -> Self {
        let timestamp = Utc::now().timestamp_millis();

        let mut hasher = Sha3_512::new();
        hasher.update(session_id.as_bytes());
        hasher.update(timestamp.to_be_bytes());
        let session_hash = format!("{:x}", hasher.finalize_reset());

        hasher.update(result.to_string_radix(10).as_bytes());
        let result_hash = format!("{:x}", hasher.finalize());

        PhaseToken { timestamp, session_hash, result_hash }
    }

    fn display(&self) {
        println!("\n{}", "🔐 PhaseToken:".bright_magenta().bold());
        println!("   ⏱️  Timestamp   : {}", self.timestamp);
        println!("   🌱 Session Hash: {}...", &self.session_hash[..32]);
        println!("   📦 Result Hash : {}...", &self.result_hash[..32]);
    }

    fn to_string_full(&self) -> String {
        format!(
            "PhaseToken\n\
             ==========\n\
             Timestamp: {}\n\
             Session Hash: {}\n\
             Result Hash: {}\n",
            self.timestamp, self.session_hash, self.result_hash
        )
    }
}

// ============================================================================
// PATTERN MATRIX (Array-based for speed)
// ============================================================================

#[derive(Clone, Debug, Serialize, Deserialize)]
struct PatternMatrix {
    // Direct array lookup: [prev_symbol + 1][next_symbol + 1] = probability
    transitions: [[f64; 4]; 4],
    sample_count: usize,
}

impl PatternMatrix {
    fn new() -> Self {
        Self {
            transitions: [[0.25; 4]; 4],
            sample_count: 0,
        }
    }

    fn learn_from_sequence(&mut self, symbols: &[i8]) {
        let mut counts = [[0usize; 4]; 4];

        for i in 0..symbols.len().saturating_sub(1) {
            let curr_idx = (symbols[i] + 1) as usize;
            let next_idx = (symbols[i + 1] + 1) as usize;

            if curr_idx < 4 && next_idx < 4 {
                counts[curr_idx][next_idx] += 1;
            }
        }

        for i in 0..4 {
            let row_sum: usize = counts[i].iter().sum();
            if row_sum > 0 {
                for j in 0..4 {
                    let old_prob = self.transitions[i][j];
                    let new_prob = counts[i][j] as f64 / row_sum as f64;
                    self.transitions[i][j] = 0.7 * new_prob + 0.3 * old_prob;
                }
            }
        }

        self.sample_count += symbols.len();
    }

    #[inline]
    fn predict_next(&self, prev_symbol: i8, rng: &mut ThreadRng) -> i8 {
        let prev_idx = (prev_symbol + 1) as usize;
        if prev_idx >= 4 {
            return *[-1, 0, 1, 2].choose(rng).unwrap();
        }

        let probs = &self.transitions[prev_idx];
        let r: f64 = rng.gen();
        let mut cumulative = 0.0;

        for (idx, &prob) in probs.iter().enumerate() {
            cumulative += prob;
            if r < cumulative {
                return (idx as i8) - 1;
            }
        }

        2
    }
}


// ============================================================================
// CONFIGURATION
// ============================================================================

#[derive(Clone)]
struct Config {
    sequence_len: usize,
    primality_rounds: u32,
    pattern_guide_ratio: f64,
    num_threads: usize,
    /// Window size as a multiple of ln(N): the expected number of primes inside it.
    window_expected_primes: f64,
}

impl Config {
    fn new(sequence_len: usize) -> Self {
        let physical_cpus = num_cpus::get_physical();
        let num_threads = physical_cpus.max(1);  // Use all cores

        Self {
            sequence_len,
            primality_rounds: 15,
            pattern_guide_ratio: 0.75,
            num_threads,
            window_expected_primes: 30.0,
        }
    }

    fn target_digits(&self) -> usize {
        (self.sequence_len as f64 * 0.602).ceil() as usize
    }
}

// ============================================================================
// STAGE 1: SYMBOLIC SEED GENERATION
// ============================================================================

#[inline]
fn generate_symbolic_guided(length: usize, pattern: &PatternMatrix, guide_ratio: f64) -> Vec<i8> {
    let mut symbols = Vec::with_capacity(length);

    THREAD_RNG.with(|rng_cell| {
        let mut rng = rng_cell.borrow_mut();

        symbols.push(*[-1, 0, 1, 2].choose(&mut *rng).unwrap());

        for _ in 1..length {
            if rng.gen::<f64>() < guide_ratio {
                let prev = *symbols.last().unwrap();
                symbols.push(pattern.predict_next(prev, &mut *rng));
            } else {
                symbols.push(*[-1, 0, 1, 2].choose(&mut *rng).unwrap());
            }
        }

        // Top symbol must be non-zero so the number really has the target size.
        if let Some(top) = symbols.last_mut() {
            if *top == 0 {
                *top = *[-1, 1, 2].choose(&mut *rng).unwrap();
            }
        }
    });

    symbols
}

// ============================================================================
// SYMBOLS <-> INTEGER (O(n) bit packing: each symbol is one base-4 digit)
// ============================================================================

/// N = Σ val_i * 4^i, with symbol -1 standing for digit 3.
/// A base-4 digit is exactly two bits, so the symbols pack straight into bytes.
fn collapse(symbols: &[i8]) -> Integer {
    let mut bytes = vec![0u8; (symbols.len() + 3) / 4];
    for (i, &s) in symbols.iter().enumerate() {
        let val = if s == -1 { 3u8 } else { s as u8 };
        bytes[i / 4] |= val << (2 * (i % 4));
    }
    Integer::from_digits(&bytes, Order::Lsf)
}

/// Inverse of `collapse`, zero-padded to at least `min_len` symbols.
fn symbols_of(n: &Integer, min_len: usize) -> Vec<i8> {
    let bytes = n.to_digits::<u8>(Order::Lsf);
    let mut symbols = Vec::with_capacity((bytes.len() * 4).max(min_len));
    for b in bytes {
        for j in 0..4 {
            let val = (b >> (2 * j)) & 3;
            symbols.push(if val == 3 { -1 } else { val as i8 });
        }
    }
    while symbols.len() > min_len && symbols.last() == Some(&0) {
        symbols.pop();
    }
    symbols.resize(symbols.len().max(min_len), 0);
    symbols
}

// ============================================================================
// STAGE 2: DEEP SIEVE OVER A WINDOW  seed, seed+2, seed+4, ...
// ============================================================================
//
// Every candidate that reaches a primality test costs one full modular
// exponentiation. Removing a candidate because a small prime p divides it costs
// one `seed mod p`. So we strike out multiples of every prime up to a bound
// chosen where those two costs break even, and only test what survives.
// The fraction of odd numbers surviving a sieve to B is about 1.123 / ln(B).

/// Odd primes below `limit` (limit <= 65536 is all the segmented sieve needs).
fn base_primes(limit: u64) -> Vec<u32> {
    let limit = limit as usize;
    let mut is_comp = vec![false; limit + 1];
    let mut out = Vec::new();
    let mut i = 3usize;
    while i <= limit {
        if !is_comp[i] {
            out.push(i as u32);
            let mut j = i * i;
            while j <= limit {
                is_comp[j] = true;
                j += 2 * i;
            }
        }
        i += 2;
    }
    out
}

/// Strike every window offset k where p divides seed + 2k.
#[inline]
fn strike(seed: &Integer, p: u32, composite: &[AtomicBool]) {
    let r = seed.mod_u(p) as u64;
    let p64 = p as u64;
    // Solve seed + 2k ≡ 0 (mod p) for the smallest k >= 0.
    let neg = if r == 0 { 0 } else { p64 - r };
    let mut k = if neg % 2 == 0 { neg / 2 } else { (neg + p64) / 2 };
    let w = composite.len() as u64;
    while k < w {
        composite[k as usize].store(true, Ordering::Relaxed);
        k += p64;
    }
}

const SEGMENT_SPAN: u64 = 1 << 21; // numbers per segment (odd-only => 1M flags)

/// Sieve the window with all odd primes in [3, bound). Parallel over segments.
fn sieve_window(seed: &Integer, composite: &[AtomicBool], bound: u64) {
    let root = (bound as f64).sqrt() as u64 + 1;
    let small = base_primes(root.max(3));
    let segments = (bound + SEGMENT_SPAN - 1) / SEGMENT_SPAN;

    (0..segments).into_par_iter().for_each(|s| {
        let lo = s * SEGMENT_SPAN;                 // even
        let hi = (lo + SEGMENT_SPAN).min(bound);
        // flags[i] <=> the odd number lo + 2i + 1 is composite
        let n_flags = ((hi - lo) / 2) as usize;
        let mut flags = vec![false; n_flags];
        for &q in &small {
            let q = q as u64;
            if q * q >= hi { break; }
            // first odd multiple of q that is >= max(q*q, lo)
            let mut m = (q * q).max(((lo + q - 1) / q) * q);
            if m % 2 == 0 { m += q; }
            let mut i = ((m - lo) / 2) as usize;
            while i < n_flags {
                flags[i] = true;
                i += q as usize;
            }
        }
        for (i, &c) in flags.iter().enumerate() {
            let p = lo + 2 * i as u64 + 1;
            if !c && p >= 3 {
                strike(seed, p as u32, composite);
            }
        }
    });
}

/// Pick the sieve bound where one more `seed mod p` costs as much as the
/// primality tests it is expected to save. Both costs are measured on this
/// machine for this number size.
fn choose_sieve_bound(seed: &Integer, ln_n: f64, test_secs: f64) -> u64 {
    // Time `seed mod p` on a few thousand word-sized moduli.
    let t = Instant::now();
    let mut sink = 0u32;
    let reps = 4000u32;
    for i in 0..reps {
        sink ^= seed.mod_u(0x7fff_0001u32.wrapping_add(2 * i));
    }
    std::hint::black_box(sink);
    let mod_secs = (t.elapsed().as_secs_f64() / reps as f64).max(1e-10);

    // tests(B) ≈ ln(N) * e^-γ / ln(B). Sieving prime p removes 1/p of them.
    // Break-even: p = tests(B) * test_secs / mod_secs. Iterate to a fixed point.
    let mut bound = 1e6f64;
    for _ in 0..8 {
        let tests = ln_n * 0.5615 / bound.ln();
        bound = (tests * test_secs / mod_secs).clamp(1e3, u32::MAX as f64);
    }
    bound as u64
}

// ============================================================================
// STAGE 3: FAST REJECT — one strong probable-prime test, base 2
// ============================================================================

/// A composite fails this with overwhelming probability, so it is the only
/// modular exponentiation almost every candidate ever costs.
fn strong_probable_prime_base2(n: &Integer) -> bool {
    let mut n_minus_1 = n.clone();
    n_minus_1 -= 1u32;
    let r = n_minus_1.find_one(0).unwrap_or(0);
    let d = n_minus_1.clone() >> r;
    mr_witness_passes(&Integer::from(2u32), &d, r, n, &n_minus_1)
}

// ============================================================================
// STAGE 4: CONFIRMATION — Miller-Rabin rounds + GMP Baillie-PSW
// ============================================================================
/// Single Miller-Rabin witness check — returns true if `a` does NOT witness compositeness.
#[inline]
fn mr_witness_passes(a: &Integer, d: &Integer, r: u32, n: &Integer, n_minus_1: &Integer) -> bool {
    if a >= n_minus_1 { return true; }
    let mut x = a.clone().pow_mod(d, n).unwrap();
    if x == 1u32 || x == *n_minus_1 { return true; }
    for _ in 0..r.saturating_sub(1) {
        x = x.pow_mod(&Integer::from(2u32), n).unwrap();
        if x == *n_minus_1 { return true; }
    }
    false
}

/// Miller-Rabin with GMP modpow + parallel witness checks via Rayon.
/// Deterministic bases 2, 3, 5 first; remaining rounds use per-thread GMP RandState.
/// All witnesses collected upfront, then checked with par_iter().all() — short-circuits
/// on first composite witness, but Rayon can schedule idle workers on the others.
fn is_prime_miller_rabin(n: &Integer, rounds: u32) -> bool {
    if *n < 2u32 { return false; }
    if *n == 2u32 || *n == 3u32 { return true; }
    if n.is_even() { return false; }

    let mut n_minus_1 = n.clone();
    n_minus_1 -= 1u32;

    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while d.is_even() {
        d >>= 1;
        r += 1;
    }

    // Collect all witnesses upfront
    let det_count = (rounds as usize).min(3);
    let mut witnesses: Vec<Integer> = [2u32, 3u32, 5u32][..det_count]
        .iter().map(|&b| Integer::from(b)).collect();

    let random_rounds = rounds.saturating_sub(det_count as u32);
    if random_rounds > 0 {
        MR_RAND.with(|rs_cell| {
            let mut rs = rs_cell.borrow_mut();
            let range = {
                let mut r = n_minus_1.clone();
                r -= 2u32;
                r
            };
            for _ in 0..random_rounds {
                let a = range.clone().random_below(&mut *rs) + 2u32;
                witnesses.push(a);
            }
        });
    }

    // Check all witnesses in parallel — par_iter().all() short-circuits on false
    witnesses.par_iter().all(|a| mr_witness_passes(a, &d, r, n, &n_minus_1))
}

/// Full confirmation, run only on the rare candidate that passed base 2.
fn confirm_prime(n: &Integer, rounds: u32) -> bool {
    // Both checks run side by side across the thread pool.
    // GMP: Baillie-PSW (strong base-2 + strong Lucas), then reps-24 extra MR rounds.
    let (mr_ok, bpsw_ok) = rayon::join(
        || is_prime_miller_rabin(n, rounds),
        || n.is_probably_prime(25) != IsPrime::No,
    );
    mr_ok && bpsw_ok
}

// ============================================================================
// TRAINING DATA LOADING
// ============================================================================

/// Parse symbols from any quanjp_*.txt prime file (the "Symbols:" section).
fn load_symbols_from_txt_files() -> Vec<Vec<i8>> {
    let mut results = Vec::new();
    let dir = std::path::Path::new(".");
    if let Ok(entries) = std::fs::read_dir(dir) {
        let mut paths: Vec<_> = entries
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|s| s.to_str()) == Some("txt")
                    && p.file_name()
                        .and_then(|s| s.to_str())
                        .map(|s| s.starts_with("quanjp_"))
                        .unwrap_or(false)
            })
            .collect();
        paths.sort(); // deterministic order
        for path in paths {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let mut after_symbols = false;
                for line in content.lines() {
                    if line.trim() == "Symbols:" {
                        after_symbols = true;
                        continue;
                    }
                    if after_symbols && line.starts_with('[') {
                        if let Ok(syms) = serde_json::from_str::<Vec<i8>>(line.trim()) {
                            results.push(syms);
                        }
                        break;
                    }
                }
            }
        }
    }
    results
}

fn load_training_data() -> Vec<Vec<i8>> {
    let mut training_data = load_symbols_from_txt_files();

    if let Ok(data) = std::fs::read_to_string("prime_1233_symbols.json") {
        if let Ok(symbols) = serde_json::from_str::<Vec<i8>>(&data) {
            training_data.push(symbols);
        }
    }

    if let Ok(data) = std::fs::read_to_string("prime_2466_symbols.json") {
        if let Ok(symbols) = serde_json::from_str::<Vec<i8>>(&data) {
            training_data.push(symbols);
        }
    }

    if training_data.is_empty() {
        training_data = vec![
            vec![-1, -1, -1, -1, -1, -1, -1, 1, 0, -1, 2, 2, 1, -1, 2, 0],
            vec![-1, 1, 0, -1, 0, 0, 0, 1, 1, -1, 1, -1, 1, 1, 1, -1],
        ];
    }

    training_data
}

fn load_training_data_with_history(conn: &Connection, target_digits: usize) -> Vec<Vec<i8>> {
    let mut training_data = Vec::new();

    // Only use database history for larger targets (8K+)
    if target_digits >= 8000 {
        training_data = load_from_history(conn, target_digits);
        if !training_data.is_empty() {
            println!("   ✓ Loaded {} historical sequences from database", training_data.len());
            return training_data;
        }
    }

    // Fallback: txt files from previous runs
    let txt_symbols = load_symbols_from_txt_files();
    if !txt_symbols.is_empty() {
        println!("   ✓ Loaded {} sequences from .txt prime files", txt_symbols.len());
        training_data.extend(txt_symbols);
        return training_data;
    }

    // Fallback to JSON files
    if let Ok(data) = std::fs::read_to_string("prime_1233_symbols.json") {
        if let Ok(symbols) = serde_json::from_str::<Vec<i8>>(&data) {
            training_data.push(symbols);
        }
    }

    if let Ok(data) = std::fs::read_to_string("prime_2466_symbols.json") {
        if let Ok(symbols) = serde_json::from_str::<Vec<i8>>(&data) {
            training_data.push(symbols);
        }
    }

    // Fallback to hardcoded defaults
    if training_data.is_empty() {
        training_data = vec![
            vec![-1, -1, -1, -1, -1, -1, -1, 1, 0, -1, 2, 2, 1, -1, 2, 0],
            vec![-1, 1, 0, -1, 0, 0, 0, 1, 1, -1, 1, -1, 1, 1, 1, -1],
        ];
    }

    training_data
}

// ============================================================================
// ONE HUNT: seed -> sieve window -> test survivors in parallel
// ============================================================================

struct HuntStats {
    sieve_bound: u64,
    window: usize,
    survivors: usize,
    tests_run: u64,
    sieve_time: std::time::Duration,
    test_time: std::time::Duration,
}

fn hunt_once(config: &Config, pattern: &PatternMatrix) -> (Option<Integer>, HuntStats) {
    // Symbolic seed, forced odd.
    let seed_symbols = generate_symbolic_guided(config.sequence_len, pattern, config.pattern_guide_ratio);
    let mut seed = collapse(&seed_symbols);
    seed.set_bit(0, true);

    let ln_n = seed.significant_bits() as f64 * std::f64::consts::LN_2;
    let window = ((config.window_expected_primes * ln_n / 2.0).ceil() as usize).max(64);

    // Time one real test on the seed itself (also a legitimate candidate).
    let t = Instant::now();
    let seed_passes = strong_probable_prime_base2(&seed);
    let test_secs = t.elapsed().as_secs_f64();

    let sieve_start = Instant::now();
    let bound = choose_sieve_bound(&seed, ln_n, test_secs);
    let composite: Vec<AtomicBool> = (0..window).map(|_| AtomicBool::new(false)).collect();
    sieve_window(&seed, &composite, bound);
    if !seed_passes {
        composite[0].store(true, Ordering::Relaxed);
    }
    let sieve_time = sieve_start.elapsed();

    let mut survivors: Vec<u32> = (0..window as u32)
        .filter(|&k| !composite[k as usize].load(Ordering::Relaxed))
        .collect();
    // Random order: no bias toward primes that follow a long gap.
    THREAD_RNG.with(|rng| survivors.shuffle(&mut *rng.borrow_mut()));

    let pb = ProgressBar::new(survivors.len() as u64);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] {pos}/{len} survivors tested | {per_sec} {msg}")
            .unwrap()
            .progress_chars("█▓▒░ "),
    );

    let tests_run = AtomicU64::new(0);
    let test_start = Instant::now();
    // The search stops the moment any candidate passes base 2, so every core is
    // free for the confirmation rounds instead of testing more survivors.
    let found = loop {
        let hit = survivors.par_iter().find_map_any(|&k| {
            let mut n = seed.clone();
            n += 2 * k as u64;
            let passed = strong_probable_prime_base2(&n);
            tests_run.fetch_add(1, Ordering::Relaxed);
            pb.inc(1);
            if passed { Some((k, n)) } else { None }
        });
        match hit {
            None => break None,
            Some((_, n)) if confirm_prime(&n, config.primality_rounds) => break Some(n),
            // A base-2 pseudoprime: astronomically rare. Drop it and keep going.
            Some((k, _)) => survivors.retain(|&x| x != k),
        }
    };
    pb.finish_and_clear();

    let stats = HuntStats {
        sieve_bound: bound,
        window,
        survivors: survivors.len(),
        tests_run: tests_run.load(Ordering::Relaxed),
        sieve_time,
        test_time: test_start.elapsed(),
    };
    (found, stats)
}

impl HuntStats {
    fn print(&self, ln_n: f64) {
        let unsieved = ln_n / 2.0;
        println!("\n{}", "📊 Pipeline Statistics:".bright_cyan().bold());
        println!("   Sieve bound:                   {}", self.sieve_bound);
        println!("   Window (odd offsets):          {}", self.window);
        println!("   Survived sieve:                {} ({:.2}%)", self.survivors,
                 self.survivors as f64 / self.window as f64 * 100.0);
        println!("   Primality tests run:           {}", self.tests_run);
        println!("   Expected with no sieve:        ~{:.0} tests per prime", unsieved);
        println!("\n{}", "⏱️  Performance:".bright_cyan().bold());
        println!("   Sieve time:                    {:.2?}", self.sieve_time);
        println!("   Test time:                     {:.2?}", self.test_time);
        println!("   Tests/sec:                     {:.2}",
                 self.tests_run as f64 / self.test_time.as_secs_f64().max(0.001));
    }
}

// ============================================================================
// MAIN
// ============================================================================

fn main() {
    println!("\n{}", "🚀 QuanJP Prime Hunter 2050 - SYMBOLIC-FIRST EDITION".bright_green().bold());
    println!("{}", "   ⚡ Symbolic seed + deep sieve + numeric verification".bright_yellow());
    println!("{}\n", "━".repeat(70).bright_blue());

    let physical_cpus = num_cpus::get_physical();
    let logical_cpus = num_cpus::get();

    println!("{}", "💻 System Detection:".bright_cyan().bold());
    println!("   Physical CPUs:  {}", physical_cpus);
    println!("   Logical CPUs:   {}", logical_cpus);
    println!("   Using threads:  {} (all cores)", physical_cpus.to_string().bright_green().bold());

    // =========================================================================
    // 🎯 TARGET SIZE:  quanjp-prime-hunter [sequence_len] [how_many_primes]
    //    digits ≈ sequence_len × 0.602.  Defaults below are used with no args.
    // =========================================================================
    let args: Vec<String> = std::env::args().collect();
    let sequence_len: usize = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(33200); // ~20,000 digits
    let target_count: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let config = Config::new(sequence_len);
    // =========================================================================

    // Only initialize DB for larger targets (8K+) to avoid overhead
    let db = if config.target_digits() >= 8000 {
        Some(init_database().expect("Failed to initialize database"))
    } else {
        None
    };

    rayon::ThreadPoolBuilder::new()
        .num_threads(config.num_threads)
        .build_global()
        .unwrap();

    println!("\n{}", "🧠 Loading & Learning Patterns...".bright_cyan().bold());
    let training_data = match &db {
        Some(connection) => load_training_data_with_history(connection, config.target_digits()),
        None => load_training_data(),
    };
    let mut pattern_matrix = PatternMatrix::new();

    for (idx, sequence) in training_data.iter().enumerate() {
        pattern_matrix.learn_from_sequence(sequence);
        println!("   ✅ Learned from sequence {} ({} symbols)", idx + 1, sequence.len());
    }

    println!("\n{}", "⚙️  Configuration:".bright_cyan().bold());
    println!("   Sequence length:        {}", config.sequence_len);
    println!("   Target digits:          ~{}", config.target_digits().to_string().bright_white().bold());
    println!("   Primes to find:         {}", target_count);
    println!("   Miller-Rabin rounds:    {} (+ Baillie-PSW)", config.primality_rounds);
    println!("   Pattern guidance:       {:.0}%", config.pattern_guide_ratio * 100.0);

    println!("\n{}", "🔧 Pipeline:".bright_cyan().bold());
    println!("   ① Symbolic seed generation (pattern-guided, forced odd)");
    println!("   ② Deep sieve over the window seed, seed+2, seed+4, ...");
    println!("      (bound auto-tuned: cost of one more small prime vs tests it saves)");
    println!("   ③ Strong probable-prime test base 2 on survivors, all cores");
    println!("   ④ Confirmation: {} Miller-Rabin rounds + Baillie-PSW", config.primality_rounds);

    println!(
        "\n{}",
        format!("🚀 Starting Prime Hunt — TARGET: {} PRIME(S) OF ~{} DIGITS", target_count, config.target_digits())
            .bright_green()
            .bold()
    );

    let mut primes_found = 0usize;
    let mut total_tests: u64 = 0;
    let hunt_start = Instant::now();

    while primes_found < target_count {
        println!(
            "\n{}",
            format!("🔍 Hunt {}/{} ...", primes_found + 1, target_count)
                .bright_cyan()
                .bold()
        );

        let now = Instant::now();
        let (result, stats) = hunt_once(&config, &pattern_matrix);
        let round_duration = now.elapsed();
        total_tests += stats.tests_run;

        match result {
            Some(prime) => {
                primes_found += 1;

                let symbols = symbols_of(&prime, config.sequence_len);
                let ent = {
                    let mut counts = [0usize; 4];
                    for &s in &symbols { counts[(s + 1) as usize] += 1; }
                    let total = symbols.len() as f64;
                    counts.iter().filter(|&&c| c > 0)
                        .map(|&c| { let p = c as f64 / total; -p * p.log2() })
                        .sum::<f64>()
                };

                let prime_str = prime.to_string_radix(10);
                let digits = prime_str.len();

                println!(
                    "\n{}",
                    format!("✅ 🎉 PRIME #{} DISCOVERED!", primes_found)
                        .bright_green()
                        .bold()
                );
                println!("\n   Digits:      {}", digits.to_string().bright_white().bold());
                println!("   Entropy:     {:.6}", ent);
                println!("   Sequence:    {}", symbols.len());
                println!("   Hunt time:   {:.2?}", round_duration);

                let session_id = format!(
                    "QuanJP-Symbolic-{}-{}-#{:02}",
                    digits,
                    Utc::now().format("%Y%m%d"),
                    primes_found
                );
                let token = PhaseToken::new(&session_id, &prime);
                token.display();

                if let Some(ref connection) = db {
                    if let Err(e) = save_discovery(
                        connection, token.timestamp, digits, ent,
                        symbols.len(), &symbols, &token.session_hash, &token.result_hash,
                    ) {
                        println!("   ⚠️  Warning: Failed to save to database: {}", e);
                    } else {
                        println!("   ✓ Saved to database");
                    }
                }

                let ln_n = prime.significant_bits() as f64 * std::f64::consts::LN_2;
                stats.print(ln_n);

                let filename = format!("quanjp_prime_{:02}_{digits}digits.txt", primes_found);
                std::fs::write(&filename, format!(
                    "QuanJP Ultimate Prime #{:02}\n\
                     ====================\n\
                     Digits: {}\n\
                     Entropy: {:.6}\n\
                     Sequence Length: {}\n\
                     \n\
                     {}\n\
                     Prime:\n{}\n\
                     \n\
                     Symbols:\n{:?}\n",
                    primes_found, digits, ent, symbols.len(),
                    token.to_string_full(), prime_str, symbols
                )).ok();

                println!("\n{} Saved to {}", "💾".bright_green(), filename);

                // Feed this prime's symbol pattern back so future hunts benefit
                pattern_matrix.learn_from_sequence(&symbols);
            }
            None => {
                println!(
                    "   ⚠️  Window exhausted ({} tests, no prime) — new seed...",
                    stats.tests_run
                );
            }
        }
    }

    let total_elapsed = hunt_start.elapsed();
    println!("\n{}", format!("🏆 ALL {} PRIME(S) FOUND!", target_count).bright_green().bold());
    println!("   Total wall time : {:.2?}", total_elapsed);
    println!("   Total tests     : {}", total_tests);
    println!(
        "   Avg per prime   : {:.2?}",
        total_elapsed / target_count as u32
    );
}
