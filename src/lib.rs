//! C syntax trees and keyword-template matching for the Argo parser prototype.
//!
//! Syntax nodes are deliberately separate from future UI/graph nodes. This
//! crate does not yet preprocess C or resolve names, types, or graph interfaces.

pub mod ast;
pub mod c_node;
pub mod c_node_role;
pub mod export;
pub mod graph;
pub mod lexer;
pub mod parser;
pub mod report;
pub mod source;
pub mod templates;

mod lowering;

pub use ast::Ast;
pub use c_node::{CNode, CNodeType};
pub use c_node_role::CNodeRole;
pub use graph::{Graph, GraphEdge, GraphNode};
pub use lexer::Lexer;
pub use parser::{ParseResult, Parser};
pub use templates::{Template, TemplateMatch};
