//! One recognizer per C keyword/construct, re-exported from this module.
//! The default list is ordered and ends with an explicit opaque fallback.
//! Matching syntax is only the first step toward executable graph templates.

mod block;
mod break_statement;
mod case_statement;
mod continue_statement;
mod contract;
mod declaration;
mod default_statement;
mod defaults;
mod do_statement;
mod else_statement;
mod enum_type;
mod expression;
mod for_statement;
mod function;
mod if_statement;
mod opaque;
mod return_statement;
mod static_assert;
mod struct_type;
mod switch_statement;
mod type_node;
mod typedef;
mod union_type;
mod variable;
mod while_statement;

pub use block::BlockTemplate;
pub use break_statement::BreakTemplate;
pub use case_statement::CaseTemplate;
pub use continue_statement::ContinueTemplate;
pub use contract::{Template, TemplateMatch};
pub use declaration::DeclarationTemplate;
pub use default_statement::DefaultTemplate;
pub use defaults::all;
pub use do_statement::DoWhileTemplate;
pub use else_statement::ElseTemplate;
pub use enum_type::EnumTemplate;
pub use expression::ExpressionTemplate;
pub use for_statement::ForTemplate;
pub use function::FunctionTemplate;
pub use if_statement::IfTemplate;
pub use opaque::OpaqueTemplate;
pub use return_statement::ReturnTemplate;
pub use static_assert::StaticAssertTemplate;
pub use struct_type::StructTemplate;
pub use switch_statement::SwitchTemplate;
pub use type_node::TypeTemplate;
pub use typedef::TypedefTemplate;
pub use union_type::UnionTemplate;
pub use variable::VariableTemplate;
pub use while_statement::WhileTemplate;
