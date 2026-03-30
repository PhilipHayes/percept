use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacyToken {
    pub text: String,
    pub lemma: String,
    pub pos: String,
    pub tag: String,
    pub dep: String,
    pub head: usize,
    pub ent_type: String,
    pub ent_iob: String,
    pub idx: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacySentence {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub tokens: Vec<SpacyToken>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacyEntity {
    pub text: String,
    pub label: String,
    pub start_char: usize,
    pub end_char: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpacyDoc {
    pub text: String,
    pub sentences: Vec<SpacySentence>,
    pub entities: Vec<SpacyEntity>,
}

#[derive(Debug)]
pub enum SpacyError {
    PythonNotFound { message: String },
    SpacyNotInstalled { message: String },
    ModelNotFound { model: String, message: String },
    ParseFailed { message: String, stderr: String },
    InvalidOutput { message: String, raw_output: String },
}

impl std::fmt::Display for SpacyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SpacyError::PythonNotFound { message } => {
                write!(
                    f,
                    "python3 not found: {message}\n  Fix: brew install python3 (macOS) or apt install python3 (Linux)"
                )
            }
            SpacyError::SpacyNotInstalled { message } => {
                write!(
                    f,
                    "spaCy is not installed: {message}\n  Fix: pip install spacy"
                )
            }
            SpacyError::ModelNotFound { model, message } => {
                write!(
                    f,
                    "spaCy model '{model}' not found: {message}\n  Fix: python3 -m spacy download {model}"
                )
            }
            SpacyError::ParseFailed { message, stderr } => {
                if stderr.is_empty() {
                    write!(f, "spaCy parse failed: {message}")
                } else {
                    write!(f, "spaCy parse failed: {message}\n  Detail: {stderr}")
                }
            }
            SpacyError::InvalidOutput { message, .. } => {
                write!(f, "spaCy returned invalid output: {message}")
            }
        }
    }
}

impl std::error::Error for SpacyError {}

/// Find the vendored spacy_parse.py script relative to the binary or manifest.
fn find_script() -> Result<PathBuf, SpacyError> {
    // Try locations relative to the current executable first.
    if let Ok(exe) = std::env::current_exe() {
        let exe_dir = exe.parent().unwrap_or_else(|| std::path::Path::new("."));

        let candidates = [
            exe_dir.join("spacy_parse.py"),
            exe_dir.join("scripts").join("spacy_parse.py"),
            exe_dir.join("../scripts/spacy_parse.py"),
            exe_dir.join("../../aq-nlp/scripts/spacy_parse.py"),
        ];

        for candidate in &candidates {
            if let Ok(p) = candidate.canonicalize() {
                return Ok(p);
            }
        }
    }

    // Compile-time fallback via CARGO_MANIFEST_DIR.
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let fallback = PathBuf::from(manifest_dir)
        .join("scripts")
        .join("spacy_parse.py");
    if fallback.exists() {
        return Ok(fallback);
    }

    Err(SpacyError::ParseFailed {
        message: "spacy_parse.py script not found".to_string(),
        stderr: String::new(),
    })
}

/// Invoke spaCy on the given text and return the parsed document.
pub fn parse_with_spacy(text: &str) -> Result<SpacyDoc, SpacyError> {
    let script = find_script()?;

    let mut child = Command::new("python3")
        .arg(&script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SpacyError::PythonNotFound {
                    message: e.to_string(),
                }
            } else {
                SpacyError::ParseFailed {
                    message: e.to_string(),
                    stderr: String::new(),
                }
            }
        })?;

    // Write text to stdin and close it.
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| SpacyError::ParseFailed {
                message: format!("failed to write stdin: {e}"),
                stderr: String::new(),
            })?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| SpacyError::ParseFailed {
            message: format!("failed to wait for process: {e}"),
            stderr: String::new(),
        })?;

    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        serde_json::from_str::<SpacyDoc>(&stdout).map_err(|e| SpacyError::InvalidOutput {
            message: e.to_string(),
            raw_output: stdout.clone(),
        })
    } else {
        match output.status.code() {
            Some(1) => {
                let msg = extract_message(&stderr);
                Err(SpacyError::SpacyNotInstalled { message: msg })
            }
            Some(2) => {
                let msg = extract_message(&stderr);
                Err(SpacyError::ModelNotFound {
                    model: "en_core_web_sm".to_string(),
                    message: msg,
                })
            }
            _ => Err(SpacyError::ParseFailed {
                message: format!("process exited with code {:?}", output.status.code()),
                stderr,
            }),
        }
    }
}

/// Extract the "message" field from a JSON error payload in stderr, or return the raw string.
fn extract_message(stderr: &str) -> String {
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(stderr.trim()) {
        if let Some(msg) = v.get("message").and_then(|m| m.as_str()) {
            return msg.to_string();
        }
    }
    stderr.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spacy_available() -> bool {
        std::process::Command::new("python3")
            .args(["-c", "import spacy; spacy.load('en_core_web_sm')"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    #[test]
    fn parse_hello_world() {
        if !spacy_available() {
            return;
        }
        let doc = parse_with_spacy("Hello world").expect("parse failed");
        assert_eq!(doc.sentences.len(), 1);
        assert_eq!(doc.sentences[0].tokens.len(), 2);
    }

    #[test]
    fn parse_entities() {
        if !spacy_available() {
            return;
        }
        let doc = parse_with_spacy("Sarah went to Paris.").expect("parse failed");
        let labels: Vec<(&str, &str)> = doc
            .entities
            .iter()
            .map(|e| (e.text.as_str(), e.label.as_str()))
            .collect();
        assert!(
            labels.contains(&("Sarah", "PERSON")),
            "expected PERSON entity for Sarah, got: {labels:?}"
        );
        assert!(
            labels.contains(&("Paris", "GPE")),
            "expected GPE entity for Paris, got: {labels:?}"
        );
    }

    #[test]
    fn parse_empty_string() {
        if !spacy_available() {
            return;
        }
        let doc = parse_with_spacy("").expect("parse failed");
        assert_eq!(doc.sentences.len(), 0);
        assert_eq!(doc.entities.len(), 0);
    }

    #[test]
    fn parse_multi_sentence() {
        if !spacy_available() {
            return;
        }
        let doc = parse_with_spacy("I am happy. She is sad.").expect("parse failed");
        assert_eq!(doc.sentences.len(), 2);
    }

    #[test]
    fn token_fields_populated() {
        if !spacy_available() {
            return;
        }
        let doc = parse_with_spacy("Sarah went to Paris.").expect("parse failed");
        assert!(!doc.sentences.is_empty());
        let token = &doc.sentences[0].tokens[0];
        assert!(!token.text.is_empty(), "text should be populated");
        assert!(!token.lemma.is_empty(), "lemma should be populated");
        assert!(!token.pos.is_empty(), "pos should be populated");
        assert!(!token.tag.is_empty(), "tag should be populated");
        assert!(!token.dep.is_empty(), "dep should be populated");
    }

    #[test]
    fn error_messages_include_remediation() {
        let err = SpacyError::PythonNotFound {
            message: "not found".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("brew install python3"),
            "PythonNotFound should include install hint: {msg}"
        );

        let err = SpacyError::SpacyNotInstalled {
            message: "no module".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("pip install spacy"),
            "SpacyNotInstalled should include install hint: {msg}"
        );

        let err = SpacyError::ModelNotFound {
            model: "en_core_web_sm".into(),
            message: "not found".into(),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("python3 -m spacy download en_core_web_sm"),
            "ModelNotFound should include download hint: {msg}"
        );
    }
}
