//! Bytes of the combined text traced back to project files.
//!
//! The snapshot's one source is the preprocessed combination of every unit, so its byte ranges
//! say nothing about the original files. Consumers that point into a checkout (the database
//! ingest, a worker reading context around a chunk) need the original file and line instead.

use std::collections::HashMap;
use std::path::Path;

use super::files::relative;
use super::lines::{Mapped, canonical};

pub struct Places {
    map: Mapped,
    /// Preprocessor file name → path relative to the project root, for files inside it.
    files: HashMap<String, String>,
}

impl Places {
    /// Keep `map`'s line table (not its text, which the snapshot already holds) and resolve each
    /// file it names once.
    pub(super) fn new(mut map: Mapped, root: &Path) -> Places {
        map.text = vec![];
        let files = map
            .files
            .iter()
            .filter_map(|f| {
                let path = canonical(f)?;
                path.starts_with(root)
                    .then(|| (f.clone(), relative(root, &path)))
            })
            .collect();
        Places { map, files }
    }

    /// The project file and 1-based line the byte at `pos` of the combined text came from;
    /// `None` for code from outside the project (system headers).
    pub fn at(&self, pos: u32) -> Option<(&str, u32)> {
        let o = self.map.origin(pos);
        self.files.get(o.file).map(|p| (p.as_str(), o.line))
    }
}
