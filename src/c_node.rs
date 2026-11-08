use rucc_diag::Span;

/// Syntactic categories, not visual graph nodes or executable boundaries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CNodeType {
    Function,
    Struct,
    Union,
    Enum,
    Typedef,
    Variable,
    Declaration,
    Parameter,
    Field,
    Enumerator,
    Type,
    Block,
    If,
    Else,
    For,
    While,
    DoWhile,
    Switch,
    Case,
    Default,
    Return,
    Break,
    Continue,
    Goto,
    Label,
    Expression,
    Initializer,
    StaticAssert,
    Empty,
    Opaque,
}

/// An owned C syntax node. Byte spans refer to the original input, unchanged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CNode {
    pub node_type: CNodeType,
    pub name: Option<String>,
    pub span: Span,
    pub children: Vec<CNode>,
}

impl CNode {
    pub fn new(node_type: CNodeType, span: Span) -> Self {
        Self {
            node_type,
            name: None,
            span,
            children: Vec::new(),
        }
    }

    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Recover exact bytes, including spelling and whitespace within the span.
    pub fn source<'a>(&self, source: &'a [u8]) -> Option<&'a [u8]> {
        source.get(self.span.lo as usize..self.span.hi as usize)
    }

    /// Preorder traversal, including this node. IDs are not persistent identity.
    pub fn walk(&self) -> Walk<'_> {
        Walk {
            pending: vec![self],
        }
    }
}

pub struct Walk<'a> {
    pending: Vec<&'a CNode>,
}

impl<'a> Iterator for Walk<'a> {
    type Item = &'a CNode;

    fn next(&mut self) -> Option<Self::Item> {
        let node = self.pending.pop()?;
        self.pending.extend(node.children.iter().rev());
        Some(node)
    }
}
