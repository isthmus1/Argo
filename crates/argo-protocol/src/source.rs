//! Source provenance. Graph records point into sealed source files by byte range; they never
//! carry reconstructed C.

use serde::{Deserialize, Serialize};

use crate::{ContentHash, FileId, InstanceId};

/// A source file's identity, carried by the graph so its hash covers the exact inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    pub id: FileId,
    /// Relative to the import root.
    pub path: String,
    pub sha256: ContentHash,
}

/// Exact source text, fetched on selection rather than subscribed with the graph (§4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceFile {
    pub id: FileId,
    pub path: String,
    pub sha256: ContentHash,
    /// Imports upload bounded UTF-8 source (§4), so text is exact.
    pub text: String,
}

impl SourceFile {
    pub fn new(id: FileId, path: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        Self {
            id,
            path: path.into(),
            sha256: crate::hash::sha256(text.as_bytes()),
            text,
        }
    }

    pub fn file_ref(&self) -> FileRef {
        FileRef {
            id: self.id,
            path: self.path.clone(),
            sha256: self.sha256.clone(),
        }
    }

    pub fn slice(&self, range: SourceRange) -> Option<&str> {
        (range.file == self.id)
            .then(|| self.text.get(range.lo as usize..range.hi as usize))
            .flatten()
    }
}

/// Half-open byte range in one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SourceRange {
    pub file: FileId,
    pub lo: u32,
    pub hi: u32,
}

impl SourceRange {
    pub fn contains(self, inner: SourceRange) -> bool {
        self.file == inner.file && self.lo <= inner.lo && inner.hi <= self.hi
    }
}

/// Where a node's code came from (§5.2). Code produced by a macro maps to its call site; code in a
/// template or fragment file maps to that file plus the instantiation that produced it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Provenance {
    /// Spelling location.
    pub range: SourceRange,
    /// Macro call site, when the code came from an expansion.
    pub expansion: Option<SourceRange>,
    /// The template/fragment instantiation, when the code came from one.
    pub instantiation: Option<InstanceId>,
    /// Structural sub-ranges: condition, body, arms and so on.
    pub parts: Vec<SyntaxPart>,
}

impl Provenance {
    pub fn at(range: SourceRange) -> Self {
        Self {
            range,
            expansion: None,
            instantiation: None,
            parts: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyntaxPart {
    pub role: SyntaxRole,
    pub range: SourceRange,
}

/// The structural role of a syntax sub-range, independent of its spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxRole {
    Signature,
    Initializer,
    Condition,
    Step,
    Selector,
    Body,
    Then,
    Else,
    CaseValue,
    CaseEnd,
    DefaultLabel,
}
