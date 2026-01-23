use rusqlite::{Connection, params};
use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let db = Connection::open("prime_history.db").expect("Failed to open database");

    // Create table if not exists
    db.execute(
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
    ).expect("Failed to create table");

    let results_dir = "results";

    if !Path::new(results_dir).exists() {
        println!("Results directory not found!");
        return;
    }

    let mut count = 0;
    for entry in fs::read_dir(results_dir).expect("Failed to read results directory") {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        let path = entry.path();
        if !path.is_file() || !path.to_string_lossy().ends_with(".txt") {
            continue;
        }

        println!("Processing: {}", path.display());

        if let Ok(content) = fs::read_to_string(&path) {
            if let Some((digits, entropy, seq_len, timestamp, session_hash, result_hash, symbols)) =
                parse_result_file(&content) {

                match db.execute(
                    "INSERT OR IGNORE INTO discoveries (timestamp, digits, entropy, sequence_length, symbols, session_hash, result_hash)
                     VALUES (?, ?, ?, ?, ?, ?, ?)",
                    params![timestamp, digits, entropy, seq_len, symbols, session_hash, result_hash],
                ) {
                    Ok(_) => {
                        println!("  ✓ Saved {}-digit prime", digits);
                        count += 1;
                    }
                    Err(e) => println!("  ✗ Error: {}", e),
                }
            } else {
                println!("  ✗ Could not parse file");
            }
        }
    }

    println!("\n✅ Ingestion complete! Processed {} discoveries", count);

    // Show what was ingested
    let mut stmt = db.prepare("SELECT COUNT(*), GROUP_CONCAT(DISTINCT digits) FROM discoveries").unwrap();
    let result = stmt.query_row([], |row| {
        Ok((row.get::<_, i32>(0)?, row.get::<_, String>(1)?))
    });

    if let Ok((total, digits_str)) = result {
        println!("Database now contains {} total discoveries:", total);
        println!("  Digit sizes: {}", digits_str);
    }
}

fn parse_result_file(content: &str) -> Option<(i32, f64, i32, i64, String, String, String)> {
    let lines: Vec<&str> = content.lines().collect();

    let mut digits = 0i32;
    let mut entropy = 0.0f64;
    let mut seq_len = 0i32;
    let mut timestamp = 0i64;
    let mut session_hash = String::new();
    let mut result_hash = String::new();
    let mut symbols = String::new();

    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();

        if trimmed.starts_with("Digits:") {
            digits = trimmed.split(':').nth(1)?.trim().parse().ok()?;
        } else if trimmed.starts_with("Entropy:") {
            entropy = trimmed.split(':').nth(1)?.trim().parse().ok()?;
        } else if trimmed.starts_with("Sequence Length:") {
            seq_len = trimmed.split(':').nth(1)?.trim().parse().ok()?;
        } else if trimmed.starts_with("Timestamp:") {
            // New format: extract timestamp if present
            if let Ok(ts) = trimmed.split(':').nth(1)?.trim().parse::<i64>() {
                timestamp = ts;
            }
        } else if trimmed.starts_with("Session Hash:") {
            session_hash = trimmed.split(':').nth(1)?.trim().to_string();
        } else if trimmed.starts_with("Result Hash:") {
            result_hash = trimmed.split(':').nth(1)?.trim().to_string();
        } else if trimmed == "Symbols:" {
            // Next line should be the array
            if i + 1 < lines.len() {
                symbols = lines[i + 1].to_string();
            }
            break;
        }
    }

    // Validate required fields
    if digits <= 0 || entropy <= 0.0 || seq_len <= 0 || symbols.is_empty() {
        return None;
    }

    // If timestamp is missing (old format), use current time
    if timestamp == 0 {
        timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
    }

    // If hashes are missing, generate placeholder ones
    if session_hash.is_empty() {
        session_hash = format!("legacy-{}-digits", digits);
    }
    if result_hash.is_empty() {
        result_hash = format!("legacy-hash-{}", digits);
    }

    Some((digits, entropy, seq_len, timestamp, session_hash, result_hash, symbols))
}
