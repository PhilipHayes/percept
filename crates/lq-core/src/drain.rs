use std::collections::HashMap;

use serde::Serialize;

/// A discovered log template pattern.
#[derive(Debug, Clone, Serialize)]
pub struct Pattern {
    /// The template with variables replaced by `<*>`.
    pub template: String,
    /// How many log entries matched this template.
    pub count: usize,
    /// Up to 3 sample raw lines.
    pub samples: Vec<String>,
}

/// Simplified Drain-inspired log template miner.
/// Groups log messages by length and prefix tree, then extracts common templates.
pub struct Drain {
    /// Clusters keyed by (message token count, first token).
    groups: HashMap<(usize, String), Vec<Cluster>>,
    /// Similarity threshold (0.0-1.0). Tokens matching above this ratio are same cluster.
    sim_threshold: f64,
}

struct Cluster {
    tokens: Vec<Token>,
    count: usize,
    samples: Vec<String>,
}

#[derive(Clone, Debug)]
enum Token {
    Constant(String),
    Variable,
}

impl Drain {
    pub fn new() -> Self {
        Self {
            groups: HashMap::new(),
            sim_threshold: 0.4,
        }
    }

    /// Process a single message, updating template clusters.
    pub fn process(&mut self, message: &str) {
        let tokens: Vec<&str> = message.split_whitespace().collect();
        if tokens.is_empty() {
            return;
        }

        let key = (tokens.len(), tokens[0].to_string());

        if let Some(clusters) = self.groups.get_mut(&key) {
            // Find best matching cluster — compute similarities first to avoid borrow conflict
            let sims: Vec<f64> = clusters
                .iter()
                .map(|cluster| {
                    if cluster.tokens.len() != tokens.len() {
                        0.0
                    } else {
                        Self::similarity_static(&cluster.tokens, &tokens)
                    }
                })
                .collect();

            let mut best_idx = None;
            let mut best_sim = 0.0f64;
            for (i, &sim) in sims.iter().enumerate() {
                if sim > best_sim {
                    best_sim = sim;
                    best_idx = Some(i);
                }
            }

            if best_sim >= self.sim_threshold {
                if let Some(idx) = best_idx {
                    let cluster = &mut clusters[idx];
                    // Merge: mark differing positions as Variable
                    for (i, tok) in tokens.iter().enumerate() {
                        match &cluster.tokens[i] {
                            Token::Constant(c) if c != tok => {
                                cluster.tokens[i] = Token::Variable;
                            }
                            _ => {}
                        }
                    }
                    cluster.count += 1;
                    if cluster.samples.len() < 3 {
                        cluster.samples.push(message.to_string());
                    }
                    return;
                }
            }

            // No match — new cluster
            clusters.push(Cluster {
                tokens: tokens
                    .iter()
                    .map(|t| Token::Constant(t.to_string()))
                    .collect(),
                count: 1,
                samples: vec![message.to_string()],
            });
        } else {
            // New group
            self.groups.insert(
                key,
                vec![Cluster {
                    tokens: tokens
                        .iter()
                        .map(|t| Token::Constant(t.to_string()))
                        .collect(),
                    count: 1,
                    samples: vec![message.to_string()],
                }],
            );
        }
    }

    /// Extract all discovered patterns, sorted by frequency descending.
    pub fn patterns(&self) -> Vec<Pattern> {
        let mut patterns: Vec<Pattern> = self
            .groups
            .values()
            .flat_map(|clusters| {
                clusters.iter().map(|c| Pattern {
                    template: c
                        .tokens
                        .iter()
                        .map(|t| match t {
                            Token::Constant(s) => s.as_str(),
                            Token::Variable => "<*>",
                        })
                        .collect::<Vec<_>>()
                        .join(" "),
                    count: c.count,
                    samples: c.samples.clone(),
                })
            })
            .collect();

        patterns.sort_by(|a, b| b.count.cmp(&a.count));
        patterns
    }

    fn similarity_static(template: &[Token], tokens: &[&str]) -> f64 {
        if template.len() != tokens.len() {
            return 0.0;
        }
        let matches = template
            .iter()
            .zip(tokens.iter())
            .filter(|(t, tok)| match t {
                Token::Constant(c) => c == *tok,
                Token::Variable => true,
            })
            .count();
        matches as f64 / template.len() as f64
    }
}

impl Default for Drain {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_similar_messages() {
        let mut drain = Drain::new();
        drain.process("Connection refused to host 10.0.0.1");
        drain.process("Connection refused to host 10.0.0.2");
        drain.process("Connection refused to host 10.0.0.3");

        let patterns = drain.patterns();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].count, 3);
        assert_eq!(patterns[0].template, "Connection refused to host <*>");
    }

    #[test]
    fn separates_different_patterns() {
        let mut drain = Drain::new();
        drain.process("Connection refused to host 10.0.0.1");
        drain.process("Connection refused to host 10.0.0.2");
        drain.process("Disk full on /dev/sda1");
        drain.process("Disk full on /dev/sdb1");

        let patterns = drain.patterns();
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn different_lengths_different_clusters() {
        let mut drain = Drain::new();
        drain.process("error on line 10");
        drain.process("error on line 20 of file main.rs");

        let patterns = drain.patterns();
        assert_eq!(patterns.len(), 2);
    }

    #[test]
    fn keeps_samples() {
        let mut drain = Drain::new();
        for i in 0..5 {
            drain.process(&format!("Request {} took 100ms", i));
        }

        let patterns = drain.patterns();
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].samples.len(), 3); // capped at 3
    }

    #[test]
    fn sorted_by_frequency() {
        let mut drain = Drain::new();
        drain.process("alpha one");
        drain.process("beta one");
        drain.process("beta two");
        drain.process("beta three");

        let patterns = drain.patterns();
        assert!(patterns[0].count >= patterns.last().unwrap().count);
    }
}
