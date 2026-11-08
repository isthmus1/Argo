use std::fmt;

/// Why a snapshot could not become ingest batches, or rows could not become a snapshot again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// No project row yet: nothing has been ingested.
    NoProject,
    /// Ingest is still in progress (or a subscription is still applying).
    Incomplete { have: u64, want: u64 },
    /// Rows that can't belong to one ingest: bad JSON, duplicate or missing records.
    Corrupt(String),
    /// One row exceeds `ingest_rows`' byte bound on its own.
    RowTooLarge { id: u64, size: usize },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::NoProject => f.write_str("no project has been ingested"),
            Error::Incomplete { have, want } => write!(f, "ingest incomplete: {have}/{want} rows"),
            Error::Corrupt(why) => write!(f, "inconsistent chunk rows: {why}"),
            Error::RowTooLarge { id, size } => write!(f, "chunk row {id} is {size} bytes alone"),
        }
    }
}

impl std::error::Error for Error {}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Corrupt(e.to_string())
    }
}
