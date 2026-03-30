use std::process::Command;

#[test]
fn index_and_search_round_trip() {
    let home = std::env::temp_dir().join("mq-test-home-roundtrip");
    let _ = std::fs::remove_dir_all(&home);

    let input = r#"[
        {"name": "verify_credentials", "description": "Checks user password against stored hash"},
        {"name": "generate_jwt", "description": "Creates a JSON web token for authenticated sessions"},
        {"name": "calculate_tax", "description": "Computes sales tax for a given order total"}
    ]"#;

    // Index
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["index", "--collection", "test-code", "--key", ".name", "--text", ".description"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();

    assert!(output.status.success(), "index failed: {}", String::from_utf8_lossy(&output.stderr));
    let index_result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(index_result["added"], 3);
    assert_eq!(index_result["total"], 3);

    // Search for authentication
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["search", "authentication login password", "--collection", "test-code", "-k", "3"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();

    assert!(output.status.success(), "search failed: {}", String::from_utf8_lossy(&output.stderr));
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!results.is_empty(), "search should return results");
    // verify_credentials should rank highest for auth query
    assert_eq!(results[0]["key"], "verify_credentials", "Top result should be verify_credentials");

    // Stats
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["stats", "--collection", "test-code"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();

    assert!(output.status.success(), "stats failed: {}", String::from_utf8_lossy(&output.stderr));
    let stats: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stats["items"], 3);
    assert_eq!(stats["dims"], 384);
    assert_eq!(stats["model"], "bge-small-en-v1.5");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn search_with_threshold_filters_results() {
    let home = std::env::temp_dir().join("mq-test-home-threshold");
    let _ = std::fs::remove_dir_all(&home);

    let input = r#"[
        {"name": "open_database_connection", "description": "Establishes a connection to PostgreSQL"},
        {"name": "compile_shader", "description": "Compiles GLSL vertex and fragment shaders for GPU rendering"}
    ]"#;

    // Index
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["index", "--collection", "test-threshold", "--key", ".name", "--text", ".description"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();
    assert!(output.status.success());

    // Search with high threshold — only strong matches
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["search", "database connection postgres", "--collection", "test-threshold", "--threshold", "0.5"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();

    assert!(output.status.success());
    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    // At threshold 0.5, database should match but shader probably won't
    for r in &results {
        assert!(r["score"].as_f64().unwrap() >= 0.5);
    }

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn invalidate_removes_entries() {
    let home = std::env::temp_dir().join("mq-test-home-invalidate");
    let _ = std::fs::remove_dir_all(&home);

    let input = r#"[
        {"name": "func_a", "description": "Does thing A"},
        {"name": "func_b", "description": "Does thing B"},
        {"name": "func_c", "description": "Does thing C"}
    ]"#;

    // Index 3 items
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["index", "--collection", "test-invalidate", "--key", ".name", "--text", ".description"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();
    assert!(output.status.success());

    // Invalidate func_a and func_c
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["invalidate", "--collection", "test-invalidate"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(b"func_a\nfunc_c\n").unwrap();
            child.wait_with_output()
        })
        .unwrap();

    assert!(output.status.success());
    let result: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["removed"], 2);
    assert_eq!(result["remaining"], 1);

    // Verify stats shows 1 item
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["stats", "--collection", "test-invalidate"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stats: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(stats["items"], 1);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn upsert_updates_existing_entries() {
    let home = std::env::temp_dir().join("mq-test-home-upsert");
    let _ = std::fs::remove_dir_all(&home);

    let input1 = r#"[{"name": "func_a", "description": "Original description"}]"#;
    let input2 = r#"[{"name": "func_a", "description": "Updated description"}]"#;

    // Index first time
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["index", "--collection", "test-upsert", "--key", ".name", "--text", ".description"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input1.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();
    assert!(output.status.success());
    let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(r["added"], 1);

    // Upsert with updated description
    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["index", "--collection", "test-upsert", "--key", ".name", "--text", ".description", "--upsert"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input2.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();
    assert!(output.status.success());
    let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(r["updated"], 1);
    assert_eq!(r["total"], 1); // still 1, not 2

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn ndjson_input_works() {
    let home = std::env::temp_dir().join("mq-test-home-ndjson");
    let _ = std::fs::remove_dir_all(&home);

    let input = r#"{"name": "func_a", "description": "Does A"}
{"name": "func_b", "description": "Does B"}"#;

    let output = Command::new(env!("CARGO_BIN_EXE_mq"))
        .env("HOME", &home)
        .args(["index", "--collection", "test-ndjson", "--key", ".name", "--text", ".description"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();

    assert!(output.status.success(), "ndjson index failed: {}", String::from_utf8_lossy(&output.stderr));
    let r: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(r["added"], 2);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn match_finds_fuzzy_pairs() {
    let home = std::env::temp_dir().join("mq-test-home-match");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    let left_file = home.join("bills.json");
    let right_file = home.join("transactions.json");

    std::fs::write(
        &left_file,
        r#"[
            {"name": "GitHub Copilot", "amount": 10},
            {"name": "Netflix Subscription", "amount": 15},
            {"name": "Electric Utility Bill", "amount": 120}
        ]"#,
    )
    .unwrap();

    std::fs::write(
        &right_file,
        r#"[
            {"description": "GITHUB.COM/COPILOT CHARGE", "total": 10},
            {"description": "NETFLIX.COM MONTHLY", "total": 15},
            {"description": "CITYPOWER ELECTRIC PAYMENT", "total": 120}
        ]"#,
    )
    .unwrap();

    let binary = env!("CARGO_BIN_EXE_mq");
    let output = Command::new(binary)
        .env("HOME", &home)
        .args([
            "match",
            left_file.to_str().unwrap(),
            right_file.to_str().unwrap(),
            "--left-key", ".name",
            "--right-key", ".description",
            "--threshold", "0.5",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "match failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let matches: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!matches.is_empty(), "Expected at least one match");

    // GitHub Copilot should match GITHUB.COM/COPILOT
    let github_match = matches.iter().find(|m| {
        m["left"].as_str().unwrap_or("").contains("GitHub")
            && m["right"].as_str().unwrap_or("").contains("GITHUB")
    });
    assert!(github_match.is_some(), "Expected GitHub Copilot to match GITHUB.COM/COPILOT");
    assert!(github_match.unwrap()["score"].as_f64().unwrap() >= 0.5);

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn similar_finds_related_items() {
    let home = std::env::temp_dir().join("mq-test-home-similar");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    let binary = env!("CARGO_BIN_EXE_mq");

    // Index a collection with related items
    let input = r#"[
        {"key": "auth-login", "text": "User authentication and login system"},
        {"key": "auth-oauth", "text": "OAuth2 social login integration"},
        {"key": "billing-pay", "text": "Payment processing and invoicing"},
        {"key": "auth-2fa", "text": "Two-factor authentication setup"},
        {"key": "billing-sub", "text": "Subscription billing management"}
    ]"#;

    let index_out = Command::new(binary)
        .env("HOME", &home)
        .args(["index", "--collection", "modules", "--key", ".key", "--text", ".text"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();

    assert!(index_out.status.success(), "index failed: {}", String::from_utf8_lossy(&index_out.stderr));

    // Find items similar to "auth-login"
    let output = Command::new(binary)
        .env("HOME", &home)
        .args(["similar", "auth-login", "--collection", "modules", "-k", "3"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "similar failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let results: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!results.is_empty(), "Expected at least one similar result");

    // auth-oauth and auth-2fa should rank higher than billing items
    let top_key = results[0]["key"].as_str().unwrap();
    assert!(
        top_key.starts_with("auth-"),
        "Expected top similar to be auth-related, got: {}",
        top_key
    );

    let _ = std::fs::remove_dir_all(&home);
}

fn index_collection(binary: &str, home: &std::path::Path, collection: &str, input: &str) {
    use std::io::Write;
    let out = Command::new(binary)
        .env("HOME", home)
        .args(["index", "--collection", collection, "--key", ".key", "--text", ".text"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
            child.wait_with_output()
        })
        .unwrap();
    assert!(out.status.success(), "index {} failed: {}", collection, String::from_utf8_lossy(&out.stderr));
}

#[test]
fn relate_finds_cross_collection_matches() {
    let home = std::env::temp_dir().join("mq-test-home-relate");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    let binary = env!("CARGO_BIN_EXE_mq");

    // Index two related collections
    index_collection(binary, &home, "errors", r#"[
        {"key": "err-auth", "text": "Authentication failed: invalid credentials"},
        {"key": "err-timeout", "text": "Request timed out after 30 seconds"},
        {"key": "err-perm", "text": "Permission denied: insufficient access rights"}
    ]"#);

    index_collection(binary, &home, "docs", r#"[
        {"key": "doc-login", "text": "How to configure user login and authentication"},
        {"key": "doc-network", "text": "Network configuration and timeout settings"},
        {"key": "doc-rbac", "text": "Role-based access control and permissions setup"}
    ]"#);

    // Relate errors to docs
    let output = Command::new(binary)
        .env("HOME", &home)
        .args(["relate", "errors", "docs", "-k", "2", "--threshold", "0.3"])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "relate failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let relations: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert!(!relations.is_empty(), "Expected at least one relation");

    // err-auth should relate to doc-login (authentication)
    let auth_rel = relations.iter().find(|r| r["key"] == "err-auth");
    assert!(auth_rel.is_some(), "Expected err-auth to have relations");
    let auth_matches = auth_rel.unwrap()["matches"].as_array().unwrap();
    assert!(
        auth_matches.iter().any(|m| m["key"] == "doc-login"),
        "Expected err-auth to match doc-login, got: {:?}",
        auth_matches
    );

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn classify_assigns_categories() {
    let home = std::env::temp_dir().join("mq-test-home-classify");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    let binary = env!("CARGO_BIN_EXE_mq");

    // Index items to classify
    index_collection(binary, &home, "tasks", r#"[
        {"key": "fix-login-bug", "text": "Fix the broken user login authentication flow"},
        {"key": "add-dark-mode", "text": "Implement dark mode theme for the user interface"},
        {"key": "optimize-query", "text": "Speed up slow database query performance"},
        {"key": "write-api-docs", "text": "Document the REST API endpoints and responses"}
    ]"#);

    // Classify against categories
    let output = Command::new(binary)
        .env("HOME", &home)
        .args([
            "classify",
            "--collection", "tasks",
            "--categories", "bug fix,feature,performance,documentation",
            "--threshold", "0.2",
        ])
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "classify failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let classified: Vec<serde_json::Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(classified.len(), 4, "Expected all 4 items classified");

    // Check each item got a reasonable category
    let find_cat = |key: &str| -> String {
        classified.iter()
            .find(|c| c["key"] == key)
            .map(|c| c["category"].as_str().unwrap().to_string())
            .unwrap_or_default()
    };

    assert_eq!(find_cat("fix-login-bug"), "bug fix");
    assert_eq!(find_cat("write-api-docs"), "documentation");

    let _ = std::fs::remove_dir_all(&home);
}

#[test]
fn relate_rejects_model_mismatch() {
    let home = std::env::temp_dir().join("mq-test-home-relate-mismatch");
    let _ = std::fs::remove_dir_all(&home);
    std::fs::create_dir_all(&home).unwrap();

    let binary = env!("CARGO_BIN_EXE_mq");

    // Index one collection
    index_collection(binary, &home, "col-a", r#"[{"key": "a", "text": "hello"}]"#);

    // Try to relate to nonexistent collection
    let output = Command::new(binary)
        .env("HOME", &home)
        .args(["relate", "col-a", "nonexistent", "-k", "1"])
        .output()
        .unwrap();

    assert!(!output.status.success(), "relate should fail for missing collection");

    let _ = std::fs::remove_dir_all(&home);
}
