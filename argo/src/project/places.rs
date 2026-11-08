//! Bytes of the recorded sources traced back to project files.
//!
//! Each unit is recorded as its preprocessed text cut down to its own project items, so its byte
//! offsets say nothing about the original files. Consumers that point into a checkout (the
//! database ingest, a worker reading context around a chunk) need the original file and line.

use std::collections::HashMap;

use argo_protocol::FileId;

#[derive(Default)]
pub struct Places {
    /// Project-relative paths, each once.
    paths: Vec<String>,
    /// Per recorded file, sorted by offset: where each line starts, its path (index into `paths`;
    /// `None` outside the project) and its 1-based line.
    files: HashMap<FileId, Vec<(u32, Option<u32>, u32)>>,
}

impl Places {
    /// The index of project-relative `path`, adding it the first time.
    pub(super) fn path(&mut self, path: &str) -> u32 {
        match self.paths.iter().position(|p| p == path) {
            Some(i) => i as u32,
            None => {
                self.paths.push(path.to_string());
                self.paths.len() as u32 - 1
            }
        }
    }

    pub(super) fn name(&self, path: u32) -> &str {
        &self.paths[path as usize]
    }

    pub(super) fn add(&mut self, file: FileId, lines: Vec<(u32, Option<u32>, u32)>) {
        debug_assert!(lines.is_sorted_by_key(|l| l.0));
        self.files.insert(file, lines);
    }

    /// The project file and 1-based line that byte `pos` of recorded `file` came from; `None` for
    /// code from outside the project (system headers) or a file it doesn't know.
    pub fn at(&self, file: FileId, pos: u32) -> Option<(&str, u32)> {
        let lines = self.files.get(&file)?;
        let i = lines.partition_point(|l| l.0 <= pos).checked_sub(1)?;
        let (_, path, line) = lines[i];
        Some((self.paths[path? as usize].as_str(), line))
    }
}
