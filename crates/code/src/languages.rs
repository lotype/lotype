//! Canonical language identifiers, human-readable titles, and common aliases.

/// Metadata for a selectable code block language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CodeLanguage {
    /// Canonical lowercase identifier stored in `attr::LANGUAGE`.
    pub id: &'static str,
    /// Human-readable label shown in the code block header and picker.
    pub label: &'static str,
    /// Common fence aliases that resolve to [`Self::id`].
    pub aliases: &'static [&'static str],
}

/// Built-in languages offered by the code block header picker.
pub const LANGUAGES: &[CodeLanguage] = &[
    CodeLanguage {
        id: "rust",
        label: "Rust",
        aliases: &["rs"],
    },
    CodeLanguage {
        id: "typescript",
        label: "TypeScript",
        aliases: &["ts", "mts", "cts"],
    },
    CodeLanguage {
        id: "tsx",
        label: "TSX",
        aliases: &[],
    },
    CodeLanguage {
        id: "javascript",
        label: "JavaScript",
        aliases: &["js", "mjs", "cjs", "jsx"],
    },
    CodeLanguage {
        id: "python",
        label: "Python",
        aliases: &["py", "pyi"],
    },
    CodeLanguage {
        id: "go",
        label: "Go",
        aliases: &["golang"],
    },
    CodeLanguage {
        id: "json",
        label: "JSON",
        aliases: &["jsonc"],
    },
    CodeLanguage {
        id: "toml",
        label: "TOML",
        aliases: &[],
    },
    CodeLanguage {
        id: "yaml",
        label: "YAML",
        aliases: &["yml"],
    },
    CodeLanguage {
        id: "bash",
        label: "Bash",
        aliases: &["sh", "shell", "zsh"],
    },
    CodeLanguage {
        id: "sql",
        label: "SQL",
        aliases: &["postgres", "sqlite"],
    },
    CodeLanguage {
        id: "html",
        label: "HTML",
        aliases: &["htm"],
    },
    CodeLanguage {
        id: "css",
        label: "CSS",
        aliases: &["scss"],
    },
    CodeLanguage {
        id: "c",
        label: "C",
        aliases: &["h"],
    },
    CodeLanguage {
        id: "cpp",
        label: "C++",
        aliases: &["c++", "cc", "cxx", "hpp"],
    },
    CodeLanguage {
        id: "java",
        label: "Java",
        aliases: &[],
    },
    CodeLanguage {
        id: "ruby",
        label: "Ruby",
        aliases: &["rb"],
    },
    CodeLanguage {
        id: "lua",
        label: "Lua",
        aliases: &[],
    },
    CodeLanguage {
        id: "zig",
        label: "Zig",
        aliases: &[],
    },
    CodeLanguage {
        id: "markdown",
        label: "Markdown",
        aliases: &["md", "mdx"],
    },
    CodeLanguage {
        id: "diff",
        label: "Diff",
        aliases: &["patch"],
    },
    CodeLanguage {
        id: "text",
        label: "Plain text",
        aliases: &["plain", "plaintext", "txt"],
    },
];

/// Resolves a fence language token or alias to its canonical name.
pub fn canonical_language(raw: &str) -> String {
    let trimmed = raw.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return "text".to_owned();
    }
    for entry in LANGUAGES {
        if entry.id == trimmed || entry.aliases.contains(&trimmed.as_str()) {
            return entry.id.to_owned();
        }
    }
    trimmed
}

/// Formats the display label for an optional code block language.
pub fn language_label(raw: Option<&str>) -> String {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return "Plain text".to_owned();
    };
    let lower = raw.to_ascii_lowercase();
    for entry in LANGUAGES {
        if entry.id == lower || entry.aliases.contains(&lower.as_str()) {
            return entry.label.to_owned();
        }
    }
    raw.to_owned()
}
