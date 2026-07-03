//! Shared test helper: one lazily-initialized EmbedEngine per test binary.
//!
//! EmbedEngine loads an ONNX model (~1s from local cache) — constructing it
//! per-test would dominate suite runtime. Each integration-test binary gets
//! exactly one engine behind a Mutex; tests lock it for the duration of any
//! embed-requiring call. Model files come from the fastembed cache (see
//! percept's mq convention: FASTEMBED_CACHE_DIR=~/.cache/fastembed).

use std::sync::{Mutex, OnceLock};

use wq_core::{EmbedEngine, ModelKind};

pub fn engine() -> &'static Mutex<EmbedEngine> {
    static ENGINE: OnceLock<Mutex<EmbedEngine>> = OnceLock::new();
    ENGINE.get_or_init(|| {
        Mutex::new(EmbedEngine::new(ModelKind::BgeSmall).expect("embedding model must load"))
    })
}
