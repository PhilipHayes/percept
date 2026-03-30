use std::io::{self, BufRead};

use anyhow::{bail, Context, Result};
use mq_embed::engine::EmbedEngine;
use mq_embed::model::{EmbeddedItem, ModelKind};
use mq_store::collection::Collection;
use mq_store::similarity::search_top_k;

fn parse_model(s: &str) -> Result<ModelKind> {
    match s {
        "bge-small" => Ok(ModelKind::BgeSmall),
        "nomic-code" => Ok(ModelKind::NomicCode),
        _ => bail!("Unknown model: '{}'. Options: bge-small, nomic-code", s),
    }
}

pub fn index(
    collection_name: &str,
    key_expr: &str,
    text_expr: Option<&str>,
    model_str: &str,
    upsert: bool,
) -> Result<()> {
    let model_kind = parse_model(model_str)?;

    // Load or create collection
    let mut coll = if Collection::exists(collection_name)? {
        let existing = Collection::load(collection_name)?;
        if existing.meta.model != model_kind {
            bail!(
                "Collection '{}' uses model {:?}, but --model={} was specified",
                collection_name,
                existing.meta.model,
                model_str
            );
        }
        existing
    } else {
        Collection::new(collection_name, model_kind)
    };

    // Read JSON from stdin
    let stdin = io::stdin();
    let input: String = stdin
        .lock()
        .lines()
        .collect::<Result<Vec<_>, _>>()?
        .join("\n");
    if input.trim().is_empty() {
        bail!("No input on stdin");
    }

    let items = parse_json_input(&input)?;

    if items.is_empty() {
        bail!("No items to index");
    }

    // Initialize embedding engine
    let mut engine = EmbedEngine::new(model_kind)?;

    // Extract keys and texts
    let mut keys = Vec::new();
    let mut texts = Vec::new();
    let mut metadatas = Vec::new();

    for item in &items {
        // Simple key extraction: treat key_expr as a top-level field name (strip leading dot)
        let field = key_expr.trim_start_matches('.');
        let key_val = item
            .get(field)
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default());
        keys.push(key_val);

        // Extract text to embed
        let text = if let Some(expr) = text_expr {
            let text_field = expr.trim_start_matches('.');
            item.get(text_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default())
        } else {
            serde_json::to_string(item).unwrap_or_default()
        };
        texts.push(text);
        metadatas.push(Some(item.clone()));
    }

    // Batch embed
    let text_refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();
    let embeddings = engine.embed_batch(&text_refs)?;

    // Upsert or add
    let existing_keys: std::collections::HashSet<String> =
        coll.items.iter().map(|i| i.key.clone()).collect();

    let mut added = 0;
    let mut updated = 0;
    for (i, embedding) in embeddings.into_iter().enumerate() {
        if existing_keys.contains(&keys[i]) {
            if upsert {
                coll.items.retain(|item| item.key != keys[i]);
                coll.add(EmbeddedItem {
                    key: keys[i].clone(),
                    embedding,
                    text: texts[i].clone(),
                    metadata: metadatas[i].clone(),
                });
                updated += 1;
            }
            // Skip duplicates when not upserting
        } else {
            coll.add(EmbeddedItem {
                key: keys[i].clone(),
                embedding,
                text: texts[i].clone(),
                metadata: metadatas[i].clone(),
            });
            added += 1;
        }
    }

    coll.save()?;

    let result = serde_json::json!({
        "collection": collection_name,
        "added": added,
        "updated": updated,
        "total": coll.meta.item_count,
    });
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

pub fn search(
    query: &str,
    collection_name: &str,
    k: usize,
    threshold: f32,
    model_str: &str,
) -> Result<()> {
    let model_kind = parse_model(model_str)?;
    let coll = Collection::load(collection_name)
        .with_context(|| format!("Collection '{}' not found", collection_name))?;

    if coll.meta.model != model_kind {
        bail!(
            "Collection '{}' uses model {:?}, but --model={} was specified",
            collection_name,
            coll.meta.model,
            model_str
        );
    }

    let mut engine = EmbedEngine::new(model_kind)?;
    let query_embedding = engine.embed_one(query)?;

    let items: Vec<(String, Vec<f32>, Option<serde_json::Value>)> = coll
        .items
        .iter()
        .map(|i| (i.key.clone(), i.embedding.clone(), i.metadata.clone()))
        .collect();

    let results = search_top_k(&query_embedding, &items, k, threshold);
    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}

pub fn stats(collection_name: &str) -> Result<()> {
    let coll = Collection::load(collection_name)
        .with_context(|| format!("Collection '{}' not found", collection_name))?;

    let path = Collection::collection_path(collection_name)?;
    let size_bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    let result = serde_json::json!({
        "collection": coll.meta.name,
        "items": coll.meta.item_count,
        "model": coll.meta.model.name(),
        "dims": coll.meta.dims,
        "size_mb": (size_bytes as f64) / (1024.0 * 1024.0),
    });
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

pub fn invalidate(collection_name: &str) -> Result<()> {
    let mut coll = Collection::load(collection_name)
        .with_context(|| format!("Collection '{}' not found", collection_name))?;

    let stdin = io::stdin();
    let keys: Vec<String> = stdin
        .lock()
        .lines()
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .collect();

    let before = coll.meta.item_count;
    coll.remove_by_keys(&keys);
    coll.save()?;

    let result = serde_json::json!({
        "collection": collection_name,
        "removed": before - coll.meta.item_count,
        "remaining": coll.meta.item_count,
    });
    println!("{}", serde_json::to_string(&result)?);
    Ok(())
}

pub fn match_cmd(
    left_path: &str,
    right_path: &str,
    left_key_expr: &str,
    right_key_expr: &str,
    threshold: f32,
    model_str: &str,
) -> Result<()> {
    let model_kind = parse_model(model_str)?;
    let mut engine = EmbedEngine::new(model_kind)?;

    // Read left items
    let left_data = if left_path == "-" {
        let stdin = io::stdin();
        stdin
            .lock()
            .lines()
            .collect::<Result<Vec<_>, _>>()?
            .join("\n")
    } else {
        std::fs::read_to_string(left_path)
            .with_context(|| format!("Failed to read left file: {}", left_path))?
    };

    // Read right items
    let right_data = std::fs::read_to_string(right_path)
        .with_context(|| format!("Failed to read right file: {}", right_path))?;

    let left_items = parse_json_input(&left_data)?;
    let right_items = parse_json_input(&right_data)?;

    let left_field = left_key_expr.trim_start_matches('.');
    let right_field = right_key_expr.trim_start_matches('.');

    // Extract text values from left and right
    let left_texts: Vec<String> = left_items
        .iter()
        .map(|item| {
            item.get(left_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default())
        })
        .collect();

    let right_texts: Vec<String> = right_items
        .iter()
        .map(|item| {
            item.get(right_field)
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| serde_json::to_string(item).unwrap_or_default())
        })
        .collect();

    // Batch embed both sides
    let left_refs: Vec<&str> = left_texts.iter().map(|s| s.as_str()).collect();
    let right_refs: Vec<&str> = right_texts.iter().map(|s| s.as_str()).collect();

    let left_embeddings = engine.embed_batch(&left_refs)?;
    let right_embeddings = engine.embed_batch(&right_refs)?;

    // Pairwise comparison
    let mut matches = Vec::new();
    for (i, left_emb) in left_embeddings.iter().enumerate() {
        for (j, right_emb) in right_embeddings.iter().enumerate() {
            let score = mq_store::similarity::cosine_similarity(left_emb, right_emb);
            if score >= threshold {
                matches.push(serde_json::json!({
                    "left": left_texts[i],
                    "right": right_texts[j],
                    "score": (score * 1000.0).round() / 1000.0,
                }));
            }
        }
    }

    // Sort by score descending
    matches.sort_by(|a, b| {
        b["score"]
            .as_f64()
            .unwrap_or(0.0)
            .partial_cmp(&a["score"].as_f64().unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    println!("{}", serde_json::to_string(&matches)?);
    Ok(())
}

pub fn similar(key: &str, collection_name: &str, k: usize, threshold: f32) -> Result<()> {
    let coll = Collection::load(collection_name)
        .with_context(|| format!("Collection '{}' not found", collection_name))?;

    // Find the item with the given key
    let source = coll.items.iter().find(|i| i.key == key).with_context(|| {
        format!(
            "Key '{}' not found in collection '{}'",
            key, collection_name
        )
    })?;

    let query_embedding = source.embedding.clone();

    // Search all items except the source itself
    let items: Vec<(String, Vec<f32>, Option<serde_json::Value>)> = coll
        .items
        .iter()
        .filter(|i| i.key != key)
        .map(|i| (i.key.clone(), i.embedding.clone(), i.metadata.clone()))
        .collect();

    let results = search_top_k(&query_embedding, &items, k, threshold);
    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}

pub fn relate(left_name: &str, right_name: &str, k: usize, threshold: f32) -> Result<()> {
    let left_coll = Collection::load(left_name)
        .with_context(|| format!("Collection '{}' not found", left_name))?;
    let right_coll = Collection::load(right_name)
        .with_context(|| format!("Collection '{}' not found", right_name))?;

    if left_coll.meta.model != right_coll.meta.model {
        bail!(
            "Model mismatch: '{}' uses {:?}, '{}' uses {:?}",
            left_name,
            left_coll.meta.model,
            right_name,
            right_coll.meta.model
        );
    }

    let right_items: Vec<(String, Vec<f32>, Option<serde_json::Value>)> = right_coll
        .items
        .iter()
        .map(|i| (i.key.clone(), i.embedding.clone(), i.metadata.clone()))
        .collect();

    let mut relations = Vec::new();
    for left_item in &left_coll.items {
        let matches = search_top_k(&left_item.embedding, &right_items, k, threshold);
        if !matches.is_empty() {
            relations.push(serde_json::json!({
                "key": left_item.key,
                "matches": matches,
            }));
        }
    }

    println!("{}", serde_json::to_string(&relations)?);
    Ok(())
}

pub fn classify(collection_name: &str, categories_csv: &str, threshold: f32) -> Result<()> {
    let coll = Collection::load(collection_name)
        .with_context(|| format!("Collection '{}' not found", collection_name))?;

    let categories: Vec<&str> = categories_csv.split(',').map(|s| s.trim()).collect();
    if categories.is_empty() {
        bail!("No categories provided");
    }

    // Embed all category labels using the collection's model
    let mut engine = EmbedEngine::new(coll.meta.model)?;
    let cat_embeddings = engine.embed_batch(&categories)?;

    let mut results = Vec::new();
    for item in &coll.items {
        let mut best_cat = "";
        let mut best_score: f32 = -1.0;

        for (ci, cat_emb) in cat_embeddings.iter().enumerate() {
            let score = mq_store::similarity::cosine_similarity(&item.embedding, cat_emb);
            if score > best_score {
                best_score = score;
                best_cat = categories[ci];
            }
        }

        if best_score >= threshold {
            results.push(serde_json::json!({
                "key": item.key,
                "category": best_cat,
                "score": (best_score * 1000.0).round() / 1000.0,
            }));
        }
    }

    println!("{}", serde_json::to_string(&results)?);
    Ok(())
}

fn parse_json_input(data: &str) -> Result<Vec<serde_json::Value>> {
    let trimmed = data.trim();
    if trimmed.starts_with('[') {
        serde_json::from_str(trimmed).context("Failed to parse JSON array")
    } else {
        trimmed
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l))
            .collect::<Result<Vec<_>, _>>()
            .context("Failed to parse NDJSON")
    }
}
