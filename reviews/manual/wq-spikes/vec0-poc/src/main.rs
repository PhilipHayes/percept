//! Spike for ADR-038 (wq) phases wq-2 / wq-3.
//!
//! Experiment 1: does vec0 accept a TEXT-typed key column (as ADR-038's
//! draft schema assumes: `node_id TEXT PRIMARY KEY`), or is it rowid-only?
//!
//! Experiment 2: does `vec0 ... MATCH ...` work against a table reached
//! through `ATTACH DATABASE`, or does it need to be in the same schema
//! sqlite-vec initialized in?
//!
//! Findings recorded in FINDINGS.md alongside this file.

use rusqlite::ffi::sqlite3_auto_extension;
use rusqlite::Connection;

fn register_vec0() {
    unsafe {
        sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    }
}

fn experiment_1_text_primary_key() {
    println!("\n=== Experiment 1: vec0 with TEXT PRIMARY KEY ===");
    let conn = Connection::open_in_memory().unwrap();

    // ADR-038's draft schema:
    // CREATE VIRTUAL TABLE node_embeddings USING vec0(
    //   node_id TEXT PRIMARY KEY,
    //   embedding FLOAT[384]
    // );
    let result = conn.execute_batch(
        "CREATE VIRTUAL TABLE node_embeddings USING vec0(
            node_id TEXT PRIMARY KEY,
            embedding FLOAT[384]
        );",
    );

    match result {
        Ok(()) => println!("PASS: vec0 accepted `node_id TEXT PRIMARY KEY` verbatim."),
        Err(e) => println!("FAIL: vec0 rejected TEXT PRIMARY KEY: {e}"),
    }
}

fn experiment_1b_knn_through_join() {
    println!("\n=== Experiment 1b: KNN query through a JOIN (TEXT PK table) ===");
    let conn = Connection::open_in_memory().unwrap();

    conn.execute_batch(
        "CREATE VIRTUAL TABLE node_embeddings USING vec0(
            node_id TEXT PRIMARY KEY,
            embedding FLOAT[384]
        );
        CREATE TABLE nodes (id TEXT PRIMARY KEY, title TEXT NOT NULL);",
    )
    .unwrap();

    let embedding: Vec<f32> = vec![0.1; 384];
    let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

    conn.execute(
        "INSERT INTO node_embeddings (node_id, embedding) VALUES (?1, ?2)",
        rusqlite::params!["node-abc-123", embedding_bytes],
    )
    .unwrap();
    conn.execute(
        "INSERT INTO nodes (id, title) VALUES ('node-abc-123', 'Fix login bug')",
        [],
    )
    .unwrap();

    let query_embedding: Vec<f32> = vec![0.1; 384];
    let query_bytes: Vec<u8> = query_embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

    // First attempt: plain LIMIT (this is what failed on the first run).
    let plain_limit = conn.query_row(
        "SELECT n.title
         FROM node_embeddings e
         JOIN nodes n ON n.id = e.node_id
         WHERE e.embedding MATCH ?1
         ORDER BY e.distance
         LIMIT 1",
        rusqlite::params![query_bytes.clone()],
        |row| row.get::<_, String>(0),
    );
    match &plain_limit {
        Ok(title) => println!("  plain `ORDER BY distance LIMIT 1` unexpectedly worked: {title}"),
        Err(e) => println!("  plain `ORDER BY distance LIMIT 1` FAILS: {e}"),
    }

    // Fix: vec0 wants an explicit `k = ?` constraint in the WHERE clause,
    // not just a LIMIT downstream of a join.
    let with_k = conn.query_row(
        "SELECT n.title
         FROM node_embeddings e
         JOIN nodes n ON n.id = e.node_id
         WHERE e.embedding MATCH ?1 AND k = 1
         ORDER BY e.distance",
        rusqlite::params![query_bytes],
        |row| row.get::<_, String>(0),
    );
    match with_k {
        Ok(title) => println!("PASS: `MATCH ?1 AND k = 1` through a JOIN works — resolved title = {title}"),
        Err(e) => println!("FAIL: `MATCH ?1 AND k = 1` through a JOIN: {e}"),
    }
}

fn experiment_2_attach_plus_match() {
    println!("\n=== Experiment 2: ATTACH DATABASE + vec0 MATCH across schemas ===");

    let dir = std::env::temp_dir().join(format!("wq-spike-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let child_db = dir.join("child.db");
    let parent_db = dir.join("parent.db");

    // Build the "child" (project) DB with its own vec0 table (TEXT PK,
    // per Experiment 1's finding) + one row.
    {
        let conn = Connection::open(&child_db).unwrap();
        conn.execute_batch(
            "CREATE VIRTUAL TABLE node_embeddings USING vec0(
                node_id TEXT PRIMARY KEY,
                embedding FLOAT[384]
            );",
        )
        .unwrap();
        let embedding: Vec<f32> = vec![0.2; 384];
        let embedding_bytes: Vec<u8> = embedding.iter().flat_map(|f| f.to_le_bytes()).collect();
        conn.execute(
            "INSERT INTO node_embeddings (node_id, embedding) VALUES ('child-node-1', ?1)",
            rusqlite::params![embedding_bytes],
        )
        .unwrap();
    }

    // Open a fresh connection (simulating the global/rollup DB), ATTACH the
    // child, and try a vec0 MATCH query against the attached schema.
    let conn = Connection::open(&parent_db).unwrap();
    conn.execute(
        &format!("ATTACH DATABASE '{}' AS child", child_db.display()),
        [],
    )
    .unwrap();

    let query_embedding: Vec<f32> = vec![0.2; 384];
    let query_bytes: Vec<u8> = query_embedding.iter().flat_map(|f| f.to_le_bytes()).collect();

    let attempt = conn.query_row(
        "SELECT node_id
         FROM child.node_embeddings
         WHERE embedding MATCH ?1 AND k = 1
         ORDER BY distance",
        rusqlite::params![query_bytes],
        |row| row.get::<_, String>(0),
    );

    match attempt {
        Ok(node_id) => println!(
            "PASS: vec0 MATCH works through ATTACH with qualified `child.node_embeddings` — resolved {node_id}"
        ),
        Err(e) => println!("FAIL: vec0 MATCH across ATTACH failed: {e}"),
    }

    let _ = std::fs::remove_dir_all(&dir);
}

fn experiment_3_max_attached() {
    println!("\n=== Experiment 3: SQLITE_MAX_ATTACHED for the bundled sqlite (rusqlite 0.31) ===");

    let dir = std::env::temp_dir().join(format!("wq-spike-attach-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();

    let conn = Connection::open(dir.join("parent.db")).unwrap();
    let mut attached = 0u32;
    for i in 0..64 {
        let child = dir.join(format!("c{i}.db"));
        // Touch the file as a valid sqlite db.
        Connection::open(&child).unwrap();
        match conn.execute(
            &format!("ATTACH DATABASE '{}' AS a{i}", child.display()),
            [],
        ) {
            Ok(_) => attached += 1,
            Err(e) => {
                println!("Attach #{} failed: {e}", i + 1);
                break;
            }
        }
    }
    println!("Max simultaneously attached DBs (beyond main): {attached}");

    let _ = std::fs::remove_dir_all(&dir);
}

fn main() {
    register_vec0();
    experiment_1_text_primary_key();
    experiment_1b_knn_through_join();
    experiment_2_attach_plus_match();
    experiment_3_max_attached();
}
