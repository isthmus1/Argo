use serde::Serialize;

/// The structural role of a syntax child, independent of its spelling or kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CNodeRole {
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
