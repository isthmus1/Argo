use super::*;

/// An ordered heterogeneous template list accepted by `Ast::match_templates`.
/// Callers may replace it or insert more-specific templates before the fallback.
pub fn all() -> Vec<Box<dyn Template>> {
    vec![
        Box::new(FunctionTemplate),
        Box::new(StructTemplate),
        Box::new(UnionTemplate),
        Box::new(EnumTemplate),
        Box::new(TypedefTemplate),
        Box::new(VariableTemplate),
        Box::new(DeclarationTemplate),
        Box::new(IfTemplate),
        Box::new(ElseTemplate),
        Box::new(ForTemplate),
        Box::new(WhileTemplate),
        Box::new(DoWhileTemplate),
        Box::new(SwitchTemplate),
        Box::new(CaseTemplate),
        Box::new(DefaultTemplate),
        Box::new(BlockTemplate),
        Box::new(ReturnTemplate),
        Box::new(BreakTemplate),
        Box::new(ContinueTemplate),
        Box::new(StaticAssertTemplate),
        Box::new(ExpressionTemplate),
        Box::new(TypeTemplate),
        Box::new(OpaqueTemplate),
    ]
}
