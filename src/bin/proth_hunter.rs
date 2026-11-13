/// Proth Hunter — PROVEN primes from structured symbol sequences.
///
/// Searches numbers of the form  N = k·4^n + 1.  In the {-1,0,1,2} alphabet that is
///
///     [ 1, 0, 0, ... 0, <head symbols of k> ]      (n symbols before the head)
///
/// Two things make this shape special:
///   1. Sieving never touches a big number: p divides N exactly when
///      k ≡ -(4^n)^-1 (mod p), which is word-sized arithmetic on k and n.
///   2. Proth's theorem: if a^((N-1)/2) ≡ -1 (mod N) for some a, N is PRIME —
///      a proof, from a single modular exponentiation.
///
/// Usage:  proth-hunter [n_symbols] [how_many_primes] [k_start]
///         digits ≈ n_symbols × 0.602.  Defaults: 4096 symbols, 1 prime.

use rand::prelude::*;
use rayon::prelude::*;
use rug::{Integer, integer::Order};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Instant;

const HEAD_SYMBOLS: u32 = 20; // k starts as a random 20-symbol (40-bit) head
const SEGMENT_SPAN: u64 = 1 << 21;
const WITNESSES: [u32; 24] = [3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97];

#[inline]
fn pow_mod(mut b: u64, mut e: u64, p: u64) -> u64 {
    let mut r = 1u64;
    b %= p;
    while e > 0 {
        if e & 1 == 1 { r = r * b % p; }
        b = b * b % p;
        e >>= 1;
    }
    r
}

/// The one residue class of k (mod p) for which p divides k·4^n + 1.
#[inline]
fn bad_residue(p: u64, n: u64) -> u64 {
    let inv2 = (p + 1) / 2;
    let inv4 = inv2 * inv2 % p;
    let inv = pow_mod(inv4, n, p); // (4^n)^-1
    if inv == 0 { 0 } else { p - inv }
}

fn base_primes(limit: u64) -> Vec<u32> {
    let limit = limit as usize;
    let mut comp = vec![false; limit + 1];
    let mut out = Vec::new();
    let mut i = 3usize;
    while i <= limit {
        if !comp[i] {
            out.push(i as u32);
            let mut j = i * i;
            while j <= limit { comp[j] = true; j += 2 * i; }
        }
        i += 2;
    }
    out
}

/// Strike every k in [k0, k0+window) for which some odd prime below `bound` divides k·4^n+1.
fn sieve(k0: u64, n: u64, composite: &[AtomicBool], bound: u64) {
    let small = base_primes(((bound as f64).sqrt() as u64 + 1).max(3));
    let w = composite.len() as u64;
    let segments = (bound + SEGMENT_SPAN - 1) / SEGMENT_SPAN;
    (0..segments).into_par_iter().for_each(|s| {
        let lo = s * SEGMENT_SPAN;
        let hi = (lo + SEGMENT_SPAN).min(bound);
        let n_flags = ((hi - lo) / 2) as usize;
        let mut flags = vec![false; n_flags];
        for &q in &small {
            let q = q as u64;
            if q * q >= hi { break; }
            let mut m = (q * q).max(((lo + q - 1) / q) * q);
            if m % 2 == 0 { m += q; }
            let mut i = ((m - lo) / 2) as usize;
            while i < n_flags { flags[i] = true; i += q as usize; }
        }
        for (i, &c) in flags.iter().enumerate() {
            let p = lo + 2 * i as u64 + 1;
            if c || p < 3 { continue; }
            let bad = bad_residue(p, n);
            let mut j = (bad + p - k0 % p) % p; // first offset with k0 + j ≡ bad
            while j < w {
                composite[j as usize].store(true, Ordering::Relaxed);
                j += p;
            }
        }
    });
}

fn proth_number(k: u64, n: u64) -> Integer {
    (Integer::from(k) << (2 * n as u32)) + 1u32
}

/// Proth test. Some(a) = PROVEN prime with witness a; None = composite.
fn proth_test(n_big: &Integer) -> Option<u32> {
    // Pick a with Jacobi(a|N) = -1. Then N is prime  <=>  a^((N-1)/2) ≡ -1 (mod N).
    let a = WITNESSES.iter().copied().find(|&a| Integer::from(a).jacobi(n_big) == -1)?;
    let mut n_minus_1 = n_big.clone();
    n_minus_1 -= 1u32;
    let half = n_minus_1.clone() >> 1;
    let x = Integer::from(a).pow_mod(&half, n_big).unwrap();
    if x == n_minus_1 { Some(a) } else { None }
}

fn symbols_of(n: &Integer) -> Vec<i8> {
    let mut out = Vec::new();
    for b in n.to_digits::<u8>(Order::Lsf) {
        for j in 0..4 {
            let v = (b >> (2 * j)) & 3;
            out.push(if v == 3 { -1 } else { v as i8 });
        }
    }
    while out.last() == Some(&0) { out.pop(); }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: u64 = args.get(1).and_then(|s| s.parse().ok()).unwrap_or(4096);
    let target: usize = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1);
    let mut rng = thread_rng();
    let k0: u64 = args.get(3).and_then(|s| s.parse().ok())
        .unwrap_or_else(|| rng.gen_range(1u64 << (2 * HEAD_SYMBOLS - 2)..1u64 << (2 * HEAD_SYMBOLS)));
    assert!(2 * n > 2 * HEAD_SYMBOLS as u64 + 24, "n too small: Proth's theorem needs k < 4^n");

    let threads = num_cpus::get_physical().max(1);
    rayon::ThreadPoolBuilder::new().num_threads(threads).build_global().unwrap();

    let first = proth_number(k0, n);
    let ln_n = first.significant_bits() as f64 * std::f64::consts::LN_2;
    let digits = (first.significant_bits() as f64 * 0.30103).ceil() as usize;
    // About 2/ln(N) of the k values give a prime; size the window for ~15 per requested prime.
    let window = ((15.0 * target as f64 * ln_n / 2.0).ceil() as usize).max(256);

    println!("\n🚀 QuanJP Proth Hunter — proven primes of the form k·4^n + 1");
    println!("   Symbols: [1, 0 × {}, head of k]   ~{} digits   threads: {}", n - 1, digits, threads);
    println!("   k window: {} .. {}   primes wanted: {}", k0, k0 + window as u64, target);

    // Cost of one test (measured on the first candidate) vs cost of one sieve prime.
    let t = Instant::now();
    let first_result = proth_test(&first);
    let test_secs = t.elapsed().as_secs_f64();
    let t = Instant::now();
    let mut sink = 0u64;
    for i in 0..20000u64 { sink ^= bad_residue(0x7fff_0001 + 2 * i, n); }
    std::hint::black_box(sink);
    let sieve_secs = (t.elapsed().as_secs_f64() / 20000.0).max(1e-10);
    let mut bound = 1e6f64;
    for _ in 0..8 {
        let tests = ln_n * 0.5615 / bound.ln();
        bound = (tests * test_secs / sieve_secs).clamp(1e3, u32::MAX as f64);
    }
    let bound = bound as u64;

    let t = Instant::now();
    let composite: Vec<AtomicBool> = (0..window).map(|_| AtomicBool::new(false)).collect();
    sieve(k0, n, &composite, bound);
    let sieve_time = t.elapsed();
    let survivors: Vec<u64> = (0..window as u64)
        .filter(|&j| !composite[j as usize].load(Ordering::Relaxed))
        .collect();
    println!("   Sieve bound {}  ->  {} of {} candidates survive ({:.2}%)  [{:.2?}]",
             bound, survivors.len(), window, survivors.len() as f64 / window as f64 * 100.0, sieve_time);
    let _ = first_result;

    let tests = AtomicU64::new(0);
    let found = AtomicU64::new(0);
    let t = Instant::now();
    let mut primes: Vec<(u64, u32)> = survivors.par_iter().filter_map(|&j| {
        if found.load(Ordering::Relaxed) >= target as u64 { return None; }
        let k = k0 + j;
        tests.fetch_add(1, Ordering::Relaxed);
        let a = proth_test(&proth_number(k, n))?;
        found.fetch_add(1, Ordering::Relaxed);
        Some((k, a))
    }).collect();
    let test_time = t.elapsed();
    primes.sort();

    for (idx, &(k, a)) in primes.iter().enumerate() {
        let p = proth_number(k, n);
        let s = p.to_string_radix(10);
        let symbols = symbols_of(&p);
        let name = format!("proth_{:02}_{}digits.txt", idx + 1, s.len());
        std::fs::write(&name, format!(
            "QuanJP Proth Prime #{:02}\n====================\nForm: {}*4^{}+1\nDigits: {}\nStatus: PROVEN (Proth's theorem, witness a = {})\nCheck: {}^((N-1)/2) = -1 (mod N)\n\nPrime:\n{}\n\nSymbols:\n{:?}\n",
            idx + 1, k, n, s.len(), a, a, s, symbols)).ok();
        println!("   ✅ PROVEN PRIME  {}·4^{}+1   ({} digits, witness {})  -> {}", k, n, s.len(), a, name);
    }
    let nt = tests.load(Ordering::Relaxed);
    println!("\n📊 {} proven prime(s), {} tests ({:.1} per prime), test time {:.2?}, {:.2} tests/sec",
             primes.len(), nt, nt as f64 / primes.len().max(1) as f64, test_time,
             nt as f64 / test_time.as_secs_f64().max(1e-6));
    println!("   Time split: sieve {:.1}%  tests {:.1}%",
             100.0 * sieve_time.as_secs_f64() / (sieve_time + test_time).as_secs_f64(),
             100.0 * test_time.as_secs_f64() / (sieve_time + test_time).as_secs_f64());
}
