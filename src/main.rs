use rand::prelude::*;
use rayon::prelude::*;
use num_bigint::{BigUint, RandBigInt};
use num_traits::{Zero, One, ToPrimitive};
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
    let conn = Connection::open("prime_history.db")?;

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
// PHASE 2: OPTIMIZED PATTERN MATRIX (Array-based for speed)
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
// CORE ALGORITHMS
// ============================================================================

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

#[inline]
fn entropy(symbols: &[i8]) -> f64 {
    let mut counts = [0usize; 4];
    for &s in symbols {
        counts[(s + 1) as usize] += 1;
    }
    let total = symbols.len() as f64;

    // Inlined entropy calculation (remove iterator overhead)
    let mut entropy = 0.0;
    for &count in &counts {
        if count > 0 {
            let p = count as f64 / total;
            entropy -= p * p.log2();
        }
    }
    entropy
}

#[inline]
fn collapse_fast(symbols: &[i8]) -> BigUint {
    let mut result = BigUint::zero();
    
    for (i, &s) in symbols.iter().enumerate() {
        let val = if s == -1 { 3u32 } else { s as u32 };
        if val > 0 && i < POWER_CACHE.len() {
            result += BigUint::from(val) * &POWER_CACHE[i];
        }
    }
    result
}

#[inline]
fn is_obviously_composite(n: &BigUint) -> bool {
    // Quick even check (bit operation is fast)
    if n.bit(0) == false {
        return true;
    }

    // Batch check: n % (2*3*5*7*11*13) = n % 30030
    // Then check if result is divisible by any small prime
    let remainder = (n % 30030u32).to_u32().unwrap_or(0);
    remainder % 3 == 0
        || remainder % 5 == 0
        || remainder % 7 == 0
        || remainder % 11 == 0
        || remainder % 13 == 0
}

#[inline]
fn enhanced_fermat_test(n: &BigUint) -> bool {
    if n < &BigUint::from(2u32) {
        return false;
    }
    
    let n_minus_1 = n - BigUint::one();
    
    for base in [2u32, 3u32] {
        let base_big = BigUint::from(base);
        if base_big.modpow(&n_minus_1, n) != BigUint::one() {
            return false;
        }
    }
    
    true
}

fn is_prime_miller_rabin(n: &BigUint, rounds: u32) -> bool {
    if n < &BigUint::from(2u32) {
        return false;
    }
    if n == &BigUint::from(2u32) || n == &BigUint::from(3u32) {
        return true;
    }
    if n.bit(0) == false {
        return false;
    }

    let n_minus_1 = n - BigUint::one();
    let mut d = n_minus_1.clone();
    let mut r = 0u32;
    while d.bit(0) == false {
        d >>= 1;
        r += 1;
    }

    // Use thread-local RNG instead of creating new one (optimization)
    THREAD_RNG.with(|rng_cell| {
        let mut rng = rng_cell.borrow_mut();

        'witness: for _ in 0..rounds {
            let a = rng.gen_biguint_range(&BigUint::from(2u32), &n_minus_1);
            let mut x = a.modpow(&d, n);

            if x == BigUint::one() || x == n_minus_1 {
                continue 'witness;
            }

            for _ in 0..r.saturating_sub(1) {
                x = x.modpow(&BigUint::from(2u32), n);
                if x == n_minus_1 {
                    continue 'witness;
                }
            }
            return false;
        }
        true
    })
}

// ============================================================================
// TRAINING & CONFIG
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
    // Smaller targets are faster with hardcoded defaults due to DB overhead
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

#[derive(Clone)]
struct Config {
    sequence_len: usize,
    entropy_threshold: f64,
    primality_rounds: u32,
    max_attempts: u64,
    pattern_guide_ratio: f64,
    num_threads: usize,
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
        }
    }
    
    fn target_digits(&self) -> usize {
        (self.sequence_len as f64 * 0.602).ceil() as usize
    }
}

struct Statistics {
    attempts: AtomicU64,
    high_entropy: AtomicU64,
    quick_composite: AtomicU64,
    fermat_filtered: AtomicU64,
    miller_rabin_tests: AtomicU64,
}

impl Statistics {
    fn new() -> Self {
        Self {
            attempts: AtomicU64::new(0),
            high_entropy: AtomicU64::new(0),
            quick_composite: AtomicU64::new(0),
            fermat_filtered: AtomicU64::new(0),
            miller_rabin_tests: AtomicU64::new(0),
        }
    }
    
    fn print_summary(&self, duration: std::time::Duration) {
        let total = self.attempts.load(Ordering::Relaxed);
        let he = self.high_entropy.load(Ordering::Relaxed);
        let qc = self.quick_composite.load(Ordering::Relaxed);
        let ff = self.fermat_filtered.load(Ordering::Relaxed);
        let mr = self.miller_rabin_tests.load(Ordering::Relaxed);
        
        println!("\n{}", "📊 Pipeline Statistics:".bright_cyan().bold());
        println!("   Total attempts:        {}", total);
        println!("   High-entropy found:    {} ({:.2}%)", he, (he as f64 / total.max(1) as f64 * 100.0));
        println!("   Quick composite:       {} ({:.2}%)", qc, (qc as f64 / he.max(1) as f64 * 100.0));
        println!("   Fermat filtered:       {} ({:.2}%)", ff, (ff as f64 / (he.saturating_sub(qc)).max(1) as f64 * 100.0));
        println!("   Miller-Rabin tests:    {}", mr);
        
        println!("\n{}", "⏱️  Performance:".bright_cyan().bold());
        println!("   Duration:              {:.2?}", duration);
        println!("   Rate:                  {:.0} attempts/sec", total as f64 / duration.as_secs_f64().max(0.001));
    }
}

// ============================================================================
// MAIN - CHANGE CONFIG HERE FOR DIFFERENT DIGIT TARGETS
// ============================================================================
//
// | Target Digits | Sequence Length              |
// |---------------|------------------------------|
// | ~2,466        | Config::new(4096)            |
// | ~4,096        | Config::new(6804)            |
// | ~4,932        | Config::new(8192)            |
// | ~8,000        | Config::new(13300)           |
// | ~10,000       | Config::new(16600)           |
// | ~50,000       | Config::new(83000)           |
// | ~100,000      | Config::new(166000)          |
//
// ============================================================================

fn main() {
    println!("\n{}", "🚀 QuanJP Prime Hunter 2050 - ULTIMATE EDITION".bright_green().bold());
    println!("{}", "   ⚡ ADI + Universal Equation + Full Optimizations".bright_yellow());
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
    let config = Config::new(6644);  // Target: ~4,000 digits
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
    println!("   Miller-Rabin rounds:    {}", config.primality_rounds);
    println!("   Max attempts:           {}", config.max_attempts);
    println!("   Pattern guidance:       {:.0}%", config.pattern_guide_ratio * 100.0);
    
    println!("\n{}", "🔧 Optimizations:".bright_cyan().bold());
    println!("   ✅ Power cache ({} entries)", POWER_CACHE.len());
    println!("   ✅ Multi-base Fermat (2,3)");
    println!("   ✅ Array patterns (O(1))");
    println!("   ✅ Quick composite (2,3,5,7,11,13)");
    
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
        
        let symbols = generate_symbolic_guided(config.sequence_len, &pattern_matrix, config.pattern_guide_ratio);
        let ent = entropy(&symbols);
        
        if ent < config.entropy_threshold {
            return None;
        }
        
        stats.high_entropy.fetch_add(1, Ordering::Relaxed);
        let n = collapse_fast(&symbols);
        
        if is_obviously_composite(&n) {
            stats.quick_composite.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        
        if !enhanced_fermat_test(&n) {
            stats.fermat_filtered.fetch_add(1, Ordering::Relaxed);
            return None;
        }
        
        stats.miller_rabin_tests.fetch_add(1, Ordering::Relaxed);
        if is_prime_miller_rabin(&n, config.primality_rounds) {
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
            let session_id = format!("QuanJP-Ultimate-{}-{}", digits, Utc::now().format("%Y%m%d"));
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
