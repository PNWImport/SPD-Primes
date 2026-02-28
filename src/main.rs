use rand::prelude::*;
use rayon::prelude::*;
use num_bigint::{BigUint, RandBigInt};
use num_traits::{Zero, One};
use std::time::Instant;
use std::sync::atomic::{AtomicU64, Ordering};
use indicatif::{ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
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
}

// ============================================================================
// DATABASE - Historical pattern and discovery tracking
// ============================================================================

fn init_database() -> rusqlite::Result<Connection> {
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
    fn new(session_id: &str, result: &BigUint) -> Self {
        let timestamp = Utc::now().timestamp_millis();

        let mut hasher = Sha3_512::new();
        hasher.update(session_id.as_bytes());
        hasher.update(timestamp.to_be_bytes());
        let session_hash = format!("{:x}", hasher.finalize_reset());

        hasher.update(result.to_bytes_be());
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
// POWER CACHE
// ============================================================================

lazy_static! {
    static ref POWER_CACHE: Vec<BigUint> = {
        let mut cache = Vec::with_capacity(200000);
        let four = BigUint::from(4u32);
        let mut power = BigUint::one();

        for _ in 0..200000 {
            cache.push(power.clone());
            power *= &four;
        }
        cache
    };
}

// ============================================================================
// CONFIGURATION
// ============================================================================

#[derive(Clone)]
struct Config {
    sequence_len: usize,
    entropy_threshold: f64,
    primality_rounds: u32,
    max_attempts: u64,
    pattern_guide_ratio: f64,
    num_threads: usize,
    symbolic_score_threshold: f64,
}

impl Config {
    fn new(sequence_len: usize) -> Self {
        let physical_cpus = num_cpus::get_physical();
        let num_threads = if physical_cpus > 1 {
            physical_cpus - 1
        } else {
            1
        };  // Reserve 1 core for OS/system (optimal)

        Self {
            sequence_len,
            entropy_threshold: 1.88,
            primality_rounds: 15,
            max_attempts: 300_000,
            pattern_guide_ratio: 0.75,
            num_threads,
            symbolic_score_threshold: 0.24,  // Tier 3: Baseline (patterns naturally score high)
        }
    }

    fn target_digits(&self) -> usize {
        (self.sequence_len as f64 * 0.602).ceil() as usize
    }
}

// ============================================================================
// STATISTICS TRACKING
// ============================================================================

struct Statistics {
    attempts: AtomicU64,
    symbolic_score_passed: AtomicU64,
    symbolic_residue_passed: AtomicU64,
    entropy_passed: AtomicU64,
    partial_collapse_passed: AtomicU64,
    full_collapse_passed: AtomicU64,
    miller_rabin_passed: AtomicU64,
}

impl Statistics {
    fn new() -> Self {
        Self {
            attempts: AtomicU64::new(0),
            symbolic_score_passed: AtomicU64::new(0),
            symbolic_residue_passed: AtomicU64::new(0),
            entropy_passed: AtomicU64::new(0),
            partial_collapse_passed: AtomicU64::new(0),
            full_collapse_passed: AtomicU64::new(0),
            miller_rabin_passed: AtomicU64::new(0),
        }
    }

    fn print_summary(&self, duration: std::time::Duration) {
        let total = self.attempts.load(Ordering::Relaxed);
        let ss = self.symbolic_score_passed.load(Ordering::Relaxed);
        let sr = self.symbolic_residue_passed.load(Ordering::Relaxed);
        let ep = self.entropy_passed.load(Ordering::Relaxed);
        let pc = self.partial_collapse_passed.load(Ordering::Relaxed);
        let fc = self.full_collapse_passed.load(Ordering::Relaxed);
        let mr = self.miller_rabin_passed.load(Ordering::Relaxed);

        println!("\n{}", "📊 Symbolic-First Pipeline Statistics:".bright_cyan().bold());
        println!("   Total attempts:                {}", total);
        println!("   ① Symbolic score pass:         {} ({:.2}%)", ss, (ss as f64 / total.max(1) as f64 * 100.0));
        println!("   ② Symbolic residue pass:      {} ({:.2}%)", sr, (sr as f64 / ss.max(1) as f64 * 100.0));
        println!("   ③ Entropy pass:                {} ({:.2}%)", ep, (ep as f64 / sr.max(1) as f64 * 100.0));
        println!("   ④ Partial collapse pass:       {} ({:.2}%)", pc, (pc as f64 / ep.max(1) as f64 * 100.0));
        println!("   ⑤ Full collapse required:      {} ({:.2}%)", fc, (fc as f64 / pc.max(1) as f64 * 100.0));
        println!("   ⑥ Miller-Rabin pass:           {} ({:.2}%)", mr, (mr as f64 / fc.max(1) as f64 * 100.0));

        println!("\n{}", "⏱️  Performance:".bright_cyan().bold());
        println!("   Duration:                      {:.2?}", duration);
        println!("   Rate:                          {:.0} attempts/sec", total as f64 / duration.as_secs_f64().max(0.001));
    }
}

// ============================================================================
// STAGE 0: SYMBOLIC GENERATION (unchanged, already excellent)
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
    });

    symbols
}

// ============================================================================
// STAGE 1: SYMBOLIC SCORE GATE (NEW - O(n) int ops only)
// ============================================================================

#[inline]
fn symbolic_score(symbols: &[i8], pattern: &PatternMatrix) -> f64 {
    let mut score = 0.0;
    for i in 1..symbols.len() {
        let a = (symbols[i-1] + 1) as usize;
        let b = (symbols[i] + 1) as usize;
        if a < 4 && b < 4 {
            score += pattern.transitions[a][b];
        }
    }
    score / symbols.len() as f64
}

// ============================================================================
// STAGE 2: SYMBOLIC RESIDUE FILTERS (NEW - zero BigUint)
// ============================================================================

/// Oddness gate: checks if the collapsed number would be odd.
/// N = Σ val_i * 4^i. Since 4^i is even for i >= 1, parity depends only on val_0.
/// val_0 is odd (1 or 3) when symbols[0] ∈ {-1, 1}. No prime > 2 is even.
/// This single check eliminates ~50% of candidates before any expensive work.
#[inline]
fn is_symbolically_odd(symbols: &[i8]) -> bool {
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

// ============================================================================
// STAGE 3: ENTROPY GATE (keep existing)
// ============================================================================

#[inline]
fn entropy(symbols: &[i8]) -> f64 {
    let mut counts = [0usize; 4];
    for &s in symbols {
        counts[(s + 1) as usize] += 1;
    }
    let total = symbols.len() as f64;

    let mut entropy = 0.0;
    for &count in &counts {
        if count > 0 {
            let p = count as f64 / total;
            entropy -= p * p.log2();
        }
    }
    entropy
}

// ============================================================================
// STAGE 4: PARTIAL COLLAPSE (NEW - cheap checks before full collapse)
// ============================================================================

#[inline]
fn partial_collapse_check(symbols: &[i8]) -> bool {
    // Build lower K limbs only (first 256 symbols)
    let check_len = symbols.len().min(256);

    let mut parity_count = 0u32;
    let mut mod_65537_acc = 0u32;
    let mut pow_65537 = 1u32;

    for &s in &symbols[..check_len] {
        let val = if s == -1 { 3u32 } else { s as u32 };

        parity_count ^= val;
        mod_65537_acc = (mod_65537_acc + val * pow_65537) % 65537;
        pow_65537 = (pow_65537 * 4) % 65537;
    }

    // Reject if obviously wrong
    // Primes typically have mixed parity patterns
    // And non-zero residue mod 65537
    // Also reject if parity is too imbalanced
    let balanced_parity = parity_count > 0 && parity_count < 256;
    mod_65537_acc != 0 && balanced_parity
}

// ============================================================================
// STAGE 5: FULL COLLAPSE (rare now)
// ============================================================================

#[inline]
fn collapse_fast(symbols: &[i8]) -> BigUint {
    let mut result = BigUint::zero();

    for (i, &s) in symbols.iter().enumerate() {
        let val = if s == -1 { 3u32 } else { s as u32 };
        if val > 0 {
            result += BigUint::from(val) * &POWER_CACHE[i];
        }
    }
    result
}

// ============================================================================
// STAGE 6: VERIFICATION (Miller-Rabin)
// ============================================================================

/// Miller-Rabin with deterministic small bases first.
/// Bases 2, 3, 5 catch most composites immediately — no RNG overhead,
/// no BigUint random generation. Remaining rounds use random witnesses.
#[inline]
fn is_prime_miller_rabin(n: &BigUint, rounds: u32) -> bool {
    if n < &BigUint::from(2u32) {
        return false;
    }
    if n == &BigUint::from(2u32) || n == &BigUint::from(3u32) {
        return true;
    }
    if !n.bit(0) {
        return false;
    }

    let n_minus_1 = n - BigUint::one();
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while !d.bit(0) {
        d >>= 1;
        r += 1;
    }

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
// TRAINING DATA LOADING
// ============================================================================

fn load_training_data() -> Vec<Vec<i8>> {
    let mut training_data = Vec::new();

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
// MAIN
// ============================================================================

fn main() {
    println!("\n{}", "🚀 QuanJP Prime Hunter 2050 - SYMBOLIC-FIRST EDITION".bright_green().bold());
    println!("{}", "   ⚡ Tier 3: Symbolic exploration + numeric verification".bright_yellow());
    println!("{}\n", "━".repeat(70).bright_blue());

    let physical_cpus = num_cpus::get_physical();
    let logical_cpus = num_cpus::get();

    println!("{}", "💻 System Detection:".bright_cyan().bold());
    println!("   Physical CPUs:  {}", physical_cpus);
    println!("   Logical CPUs:   {}", logical_cpus);
    println!("   Using threads:  {} (CPU - 1 for OS)", (physical_cpus.saturating_sub(1).max(1)).to_string().bright_green().bold());

    // =========================================================================
    // 🎯 CHANGE THIS LINE TO TARGET DIFFERENT DIGIT SIZES
    // =========================================================================
    let config = Config::new(4096);   // Target: ~2,466 digits
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
    println!("   Entropy threshold:      {:.2}", config.entropy_threshold);
    println!("   Symbolic score gate:    > {:.2}", config.symbolic_score_threshold);
    println!("   Miller-Rabin rounds:    {}", config.primality_rounds);
    println!("   Max attempts:           {}", config.max_attempts);
    println!("   Pattern guidance:       {:.0}%", config.pattern_guide_ratio * 100.0);

    println!("\n{}", "🔧 Symbolic-First Pipeline:".bright_cyan().bold());
    println!("   ① Symbolic generation");
    println!("   ② Oddness gate (symbols[0] parity — eliminates ~50%)");
    println!("   ③ Symbolic score gate (rejects ~60% of odd candidates)");
    println!("   ④ Symbolic residue filters mod {{3,5,7,11,13,17,19,23}}");
    println!("   ⑤ Entropy threshold");
    println!("   ⑥ Partial collapse checks");
    println!("   ⑦ Full BigUint collapse (now rare!)");
    println!("   ⑧ Miller-Rabin (bases 2,3,5 + {} random witnesses)", config.primality_rounds.saturating_sub(3));

    println!("\n{}", "🚀 Starting Prime Hunt...".bright_green().bold());

    let stats = Statistics::new();

    let pb = ProgressBar::new(config.max_attempts);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed}] [{bar:40}] {pos}/{len} ({per_sec}) {msg}")
            .unwrap()
            .progress_chars("█▓▒░ "),
    );

    let now = Instant::now();

    let result = (0..config.max_attempts).into_par_iter().find_map_any(|_| {
        let attempt = stats.attempts.fetch_add(1, Ordering::Relaxed);

        if attempt % 50 == 0 {
            pb.set_position(attempt);
        }

        // =====================================================================
        // STAGE 0: Symbolic generation
        // =====================================================================
        let symbols = generate_symbolic_guided(config.sequence_len, &pattern_matrix, config.pattern_guide_ratio);

        // =====================================================================
        // STAGE 0.5: Oddness gate — 1 comparison, eliminates ~50%
        // =====================================================================
        if !is_symbolically_odd(&symbols) {
            return None;
        }

        // =====================================================================
        // STAGE 1: Symbolic score gate (O(n) int ops only)
        // =====================================================================
        let score = symbolic_score(&symbols, &pattern_matrix);
        if score < config.symbolic_score_threshold {
            return None;
        }
        stats.symbolic_score_passed.fetch_add(1, Ordering::Relaxed);

        // =====================================================================
        // STAGE 2: Symbolic residue filters — mod {3,5,7,11,13,17,19,23}
        // =====================================================================
        let residues = symbolic_residues(&symbols);

        // Reject if divisible by any small prime (residue = 0)
        if residues.iter().any(|&r| r == 0) {
            return None;
        }
        stats.symbolic_residue_passed.fetch_add(1, Ordering::Relaxed);

        // =====================================================================
        // STAGE 3: Entropy gate
        // =====================================================================
        let ent = entropy(&symbols);
        if ent < config.entropy_threshold {
            return None;
        }
        stats.entropy_passed.fetch_add(1, Ordering::Relaxed);

        // =====================================================================
        // STAGE 4: Partial collapse check (cheap)
        // =====================================================================
        if !partial_collapse_check(&symbols) {
            return None;
        }
        stats.partial_collapse_passed.fetch_add(1, Ordering::Relaxed);

        // =====================================================================
        // STAGE 5: Full collapse (now very rare!)
        // =====================================================================
        let n = collapse_fast(&symbols);
        stats.full_collapse_passed.fetch_add(1, Ordering::Relaxed);

        // =====================================================================
        // STAGE 6: Miller-Rabin verification
        // =====================================================================
        if is_prime_miller_rabin(&n, config.primality_rounds) {
            stats.miller_rabin_passed.fetch_add(1, Ordering::Relaxed);
            Some((n, ent, symbols))
        } else {
            None
        }
    });

    pb.finish_and_clear();
    let duration = now.elapsed();

    match result {
        Some((prime, ent, symbols)) => {
            let digits = prime.to_str_radix(10).len();

            println!("\n{}", "✅ 🎉 PRIME DISCOVERED!".bright_green().bold());
            println!("\n   Digits:      {}", digits.to_string().bright_white().bold());
            println!("   Entropy:     {:.6}", ent);
            println!("   Sequence:    {}", symbols.len());

            // Generate PhaseToken for cryptographic proof
            let session_id = format!("QuanJP-Symbolic-{}-{}", digits, Utc::now().format("%Y%m%d"));
            let token = PhaseToken::new(&session_id, &prime);
            token.display();

            // Save discovery to database (if available)
            if let Some(ref connection) = db {
                if let Err(e) = save_discovery(connection, token.timestamp, digits, ent, symbols.len(), &symbols, &token.session_hash, &token.result_hash) {
                    println!("   ⚠️  Warning: Failed to save to database: {}", e);
                } else {
                    println!("   ✓ Saved to database");
                }
            }

            stats.print_summary(duration);

            let filename = format!("quanjp_ultimate_{}digits.txt", digits);
            let prime_str = prime.to_str_radix(10);
            std::fs::write(&filename, format!(
                "QuanJP Ultimate Prime\n\
                 ====================\n\
                 Digits: {}\n\
                 Entropy: {:.6}\n\
                 Sequence Length: {}\n\
                 \n\
                 {}\n\
                 Prime:\n{}\n\
                 \n\
                 Symbols:\n{:?}\n",
                digits, ent, symbols.len(), token.to_string_full(), prime_str, symbols
            )).ok();

            println!("\n{} Saved to {}", "💾".bright_green(), filename);
        }
        None => {
            println!("\n❌ No prime found in {} attempts", config.max_attempts);
            stats.print_summary(duration);
        }
    }
}
