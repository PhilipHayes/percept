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
            Self::Dart => tree_sitter_dart::language(),
            Self::Swift => tree_sitter_swift::LANGUAGE.into(),
        }
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
        }
    }
}
