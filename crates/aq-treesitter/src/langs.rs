use std::path::Path;

/// Supported languages and their tree-sitter grammars.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Language {
    Rust,
    JavaScript,
    TypeScript,
    TypeScriptTsx,
    Python,
    Json,
    Go,
    Java,
    C,
    Cpp,
    Dart,
    Swift,
    Kotlin,
    Toml,
    Yaml,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "rs" => Some(Self::Rust),
            "js" | "mjs" | "cjs" | "jsx" => Some(Self::JavaScript),
            "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::TypeScriptTsx),
            "py" | "pyi" => Some(Self::Python),
            "json" => Some(Self::Json),
            "go" => Some(Self::Go),
            "java" => Some(Self::Java),
            "c" | "h" => Some(Self::C),
            "cc" | "cpp" | "cxx" | "hpp" | "hxx" | "hh" => Some(Self::Cpp),
            "dart" => Some(Self::Dart),
            "swift" => Some(Self::Swift),
            "kt" | "kts" => Some(Self::Kotlin),
            "toml" => Some(Self::Toml),
            "yml" => Some(Self::Yaml),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// Detect language from a language name string (for --lang flag).
    pub fn from_name(name: &str) -> Option<Self> {
        match name.to_lowercase().as_str() {
            "rust" | "rs" => Some(Self::Rust),
            "javascript" | "js" => Some(Self::JavaScript),
            "typescript" | "ts" => Some(Self::TypeScript),
            "tsx" => Some(Self::TypeScriptTsx),
            "python" | "py" => Some(Self::Python),
            "json" => Some(Self::Json),
            "go" | "golang" => Some(Self::Go),
            "java" => Some(Self::Java),
            "c" => Some(Self::C),
            "cpp" | "c++" | "cxx" => Some(Self::Cpp),
            "dart" => Some(Self::Dart),
            "swift" => Some(Self::Swift),
            "kotlin" | "kt" => Some(Self::Kotlin),
            "toml" => Some(Self::Toml),
            "yaml" => Some(Self::Yaml),
            _ => None,
        }
    }

    /// Detect language from a file path.
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::from_extension)
    }

    /// Get the tree-sitter Language for this variant.
    pub fn ts_language(&self) -> tree_sitter::Language {
        match self {
            Self::Rust => tree_sitter_rust::LANGUAGE.into(),
            Self::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Self::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Self::TypeScriptTsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Self::Python => tree_sitter_python::LANGUAGE.into(),
            Self::Json => tree_sitter_json::LANGUAGE.into(),
            Self::Go => tree_sitter_go::LANGUAGE.into(),
            Self::Java => tree_sitter_java::LANGUAGE.into(),
            Self::C => tree_sitter_c::LANGUAGE.into(),
            Self::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            Self::Dart => tree_sitter_dart::LANGUAGE.into(),
            Self::Swift => tree_sitter_swift::LANGUAGE.into(),
            Self::Kotlin => tree_sitter_kotlin_ng::LANGUAGE.into(),
            Self::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            Self::Yaml => tree_sitter_yaml::LANGUAGE.into(),
        }
    }

    /// Named, visible node kinds of this grammar: the kinds `aq`'s trees contain
    /// (anonymous tokens such as `class` or `(` are not among them, and `ERROR`
    /// is not either — it is added by the query checker). Sorted, unique.
    pub fn node_kinds(&self) -> Vec<String> {
        self.kinds_where(|ts, id| ts.node_kind_is_named(id) && ts.node_kind_is_visible(id))
    }

    /// Kinds that exist in the grammar only as anonymous tokens (`class`, `(`):
    /// present with `--all-nodes`, never in the default tree. Sorted, unique.
    /// A name that is also a named kind (JS `string`) is listed by `node_kinds`
    /// and may be listed here too; callers treat a node kind as a node.
    pub fn anonymous_kinds(&self) -> Vec<String> {
        self.kinds_where(|ts, id| !ts.node_kind_is_named(id))
    }

    /// Every field name of the grammar (`name`, `body`, ...). tree-sitter
    /// exposes this global table, not which kinds carry which fields. Sorted, unique.
    pub fn field_names(&self) -> Vec<String> {
        let ts = self.ts_language();
        let mut names: Vec<String> = (1..=ts.field_count() as u16)
            .filter_map(|id| ts.field_name_for_id(id))
            .map(str::to_string)
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn kinds_where(&self, keep: impl Fn(&tree_sitter::Language, u16) -> bool) -> Vec<String> {
        let ts = self.ts_language();
        let mut kinds: Vec<String> = (0..ts.node_kind_count() as u16)
            .filter(|&id| keep(&ts, id))
            .filter_map(|id| ts.node_kind_for_id(id))
            .filter(|name| !name.is_empty())
            .map(str::to_string)
            .collect();
        kinds.sort();
        kinds.dedup();
        kinds
    }

    /// Get the display name for this language.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Rust => "Rust",
            Self::JavaScript => "JavaScript",
            Self::TypeScript => "TypeScript",
            Self::TypeScriptTsx => "TypeScript (TSX)",
            Self::Python => "Python",
            Self::Json => "JSON",
            Self::Go => "Go",
            Self::Java => "Java",
            Self::C => "C",
            Self::Cpp => "C++",
            Self::Dart => "Dart",
            Self::Swift => "Swift",
            Self::Kotlin => "Kotlin",
            Self::Toml => "toml",
            Self::Yaml => "yaml",
        }
    }
}

/// What `aq_core::query::check` needs to validate a query against this grammar.
impl aq_core::query::check::Grammar for Language {
    fn name(&self) -> &str {
        Language::name(self)
    }
    fn node_kinds(&self) -> Vec<String> {
        Language::node_kinds(self)
    }
    fn anonymous_kinds(&self) -> Vec<String> {
        Language::anonymous_kinds(self)
    }
    fn field_names(&self) -> Vec<String> {
        Language::field_names(self)
    }
}
