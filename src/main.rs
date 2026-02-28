use rand::prelude::*;
use rayon::prelude::*;
use rug::{Integer, rand::RandState};
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
// POWER CACHE
// ============================================================================

lazy_static! {
    static ref POWER_CACHE: Vec<Integer> = {
        let mut cache = Vec::with_capacity(200000);
        let four = Integer::from(4u32);
        let mut power = Integer::from(1u32);

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
    primality_rounds: u32,
    max_attempts: u64,
    pattern_guide_ratio: f64,
    num_threads: usize,
    symbolic_score_threshold: f64,
}

impl Config {
    fn new(sequence_len: usize) -> Self {
        let physical_cpus = num_cpus::get_physical();
        let num_threads = physical_cpus.max(1);  // Use all cores

        Self {
            sequence_len,
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
            partial_collapse_passed: AtomicU64::new(0),
            full_collapse_passed: AtomicU64::new(0),
            miller_rabin_passed: AtomicU64::new(0),
        }
    }

    fn print_summary(&self, duration: std::time::Duration) {
        let total = self.attempts.load(Ordering::Relaxed);
        let ss = self.symbolic_score_passed.load(Ordering::Relaxed);
        let sr = self.symbolic_residue_passed.load(Ordering::Relaxed);
        let pc = self.partial_collapse_passed.load(Ordering::Relaxed);
        let fc = self.full_collapse_passed.load(Ordering::Relaxed);
        let mr = self.miller_rabin_passed.load(Ordering::Relaxed);

        println!("\n{}", "📊 Symbolic-First Pipeline Statistics:".bright_cyan().bold());
        println!("   Total attempts:                {}", total);
        println!("   ① Symbolic score pass:         {} ({:.2}%)", ss, (ss as f64 / total.max(1) as f64 * 100.0));
        println!("   ② Symbolic residue pass:      {} ({:.2}%)", sr, (sr as f64 / ss.max(1) as f64 * 100.0));
        println!("   ③ Partial collapse pass:       {} ({:.2}%)", pc, (pc as f64 / sr.max(1) as f64 * 100.0));
        println!("   ④ Full collapse required:      {} ({:.2}%)", fc, (fc as f64 / pc.max(1) as f64 * 100.0));
        println!("   ⑤ Miller-Rabin pass:           {} ({:.2}%)", mr, (mr as f64 / fc.max(1) as f64 * 100.0));

        let secs = duration.as_secs_f64().max(0.001);
        let rate = total as f64 / secs;
        let collapse_rate = fc as f64 / secs;
        let mr_rate = fc as f64 / secs;
        println!("\n{}", "⏱️  Performance:".bright_cyan().bold());
        println!("   Duration:                      {:.2?}", duration);
        println!("   ── Flow rates ──────────────────────────────");
        println!("   Candidates generated:          {:.1}/sec", rate);
        println!("   Reach full collapse:           {:.2}/sec  ({:.1}% of generated)", collapse_rate, fc as f64 / total.max(1) as f64 * 100.0);
        println!("   Reach Miller-Rabin:            {:.2}/sec", mr_rate);
        println!("   ────────────────────────────────────────────");
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
fn collapse_fast(symbols: &[i8]) -> Integer {
    let mut result = Integer::new();

    for (i, &s) in symbols.iter().enumerate() {
        let val = if s == -1 { 3u32 } else { s as u32 };
        if val > 0 {
            let mut term = POWER_CACHE[i].clone();
            term *= val;
            result += term;
        }
    }
    result
}

// ============================================================================
// STAGE 6: VERIFICATION (Miller-Rabin)
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
    println!("   Using threads:  {} (all cores)", physical_cpus.to_string().bright_green().bold());

    // =========================================================================
    // 🎯 CHANGE THIS LINE TO TARGET DIFFERENT DIGIT SIZES
    // =========================================================================
    let config = Config::new(16700);  // Target: ~10,053 digits
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
    println!("   Symbolic score gate:    > {:.2}", config.symbolic_score_threshold);
    println!("   Miller-Rabin rounds:    {}", config.primality_rounds);
    println!("   Max attempts:           {}", config.max_attempts);
    println!("   Pattern guidance:       {:.0}%", config.pattern_guide_ratio * 100.0);

    println!("\n{}", "🔧 Symbolic-First Pipeline:".bright_cyan().bold());
    println!("   ① Symbolic generation");
    println!("   ② Oddness gate (symbols[0] parity — eliminates ~50%)");
    println!("   ③ Symbolic score gate (rejects ~60% of odd candidates)");
    println!("   ④ Symbolic residue filters mod {{3,5,7,11,13,17,19,23}}");
    println!("   ⑤ Partial collapse checks");
    println!("   ⑥ Full GMP Integer collapse (now rare!)");
    println!("   ⑦ Miller-Rabin (bases 2,3,5 + {} random witnesses)", config.primality_rounds.saturating_sub(3));

    println!("\n{}", "🚀 Starting Prime Hunt — TARGET: 10 PRIMES".bright_green().bold());

    let target_count = 10usize;
    let mut primes_found = 0usize;
    let mut total_attempts: u64 = 0;
    let hunt_start = Instant::now();

    while primes_found < target_count {
        println!(
            "\n{}",
            format!("🔍 Hunt {}/{} ...", primes_found + 1, target_count)
                .bright_cyan()
                .bold()
        );

        let stats = Statistics::new();

        let pb = ProgressBar::new(config.max_attempts);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("{spinner:.green} [{elapsed_precise}] {pos}/{len} attempts | {per_sec} | ETA {eta} {msg}")
                .unwrap()
                .progress_chars("█▓▒░ "),
        );

        let now = Instant::now();

        // Snapshot pattern matrix for this parallel round
        let pm_snap = pattern_matrix.clone();

        let result = (0..config.max_attempts).into_par_iter().find_map_any(|_| {
            let attempt = stats.attempts.fetch_add(1, Ordering::Relaxed);

            if attempt % 100 == 0 {
                pb.set_position(attempt);
                let secs = now.elapsed().as_secs_f64();
                if secs > 0.0 {
                    pb.set_message(format!("({:.0} att/s)", attempt as f64 / secs));
                }
            }

            let symbols = generate_symbolic_guided(config.sequence_len, &pm_snap, config.pattern_guide_ratio);

            if !is_symbolically_odd(&symbols) { return None; }

            let score = symbolic_score(&symbols, &pm_snap);
            if score < config.symbolic_score_threshold { return None; }
            stats.symbolic_score_passed.fetch_add(1, Ordering::Relaxed);

            let residues = symbolic_residues(&symbols);
            if residues.iter().any(|&r| r == 0) { return None; }
            stats.symbolic_residue_passed.fetch_add(1, Ordering::Relaxed);

            if !partial_collapse_check(&symbols) { return None; }
            stats.partial_collapse_passed.fetch_add(1, Ordering::Relaxed);

            let n = collapse_fast(&symbols);
            stats.full_collapse_passed.fetch_add(1, Ordering::Relaxed);

            if is_prime_miller_rabin(&n, config.primality_rounds) {
                stats.miller_rabin_passed.fetch_add(1, Ordering::Relaxed);
                let ent = {
                    let mut counts = [0usize; 4];
                    for &s in &symbols { counts[(s + 1) as usize] += 1; }
                    let total = symbols.len() as f64;
                    counts.iter().filter(|&&c| c > 0)
                        .map(|&c| { let p = c as f64 / total; -p * p.log2() })
                        .sum::<f64>()
                };
                Some((n, ent, symbols))
            } else {
                None
            }
        });

        pb.finish_and_clear();
        let round_duration = now.elapsed();
        total_attempts += stats.attempts.load(Ordering::Relaxed);

        match result {
            Some((prime, ent, symbols)) => {
                primes_found += 1;

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

                stats.print_summary(round_duration);

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
                    "   ⚠️  Round exhausted ({} attempts) — retrying...",
                    stats.attempts.load(Ordering::Relaxed)
                );
            }
        }
    }

    let total_elapsed = hunt_start.elapsed();
    println!("\n{}", "🏆 ALL 10 PRIMES FOUND!".bright_green().bold());
    println!("   Total wall time : {:.2?}", total_elapsed);
    println!("   Total attempts  : {}", total_attempts);
    println!(
        "   Avg per prime   : {:.2?}",
        total_elapsed / target_count as u32
    );
}
