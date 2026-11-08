use main::graph::{ChildRegion, ControlExit, ControlExitKind, Expansion, GraphNodeKind};
use main::graph::{SourceSpan, SyntaxPart, Unsupported};
use main::{CNode, CNodeRole, Parser, Template};

struct OutsideRegion(u8);
impl Template for OutsideRegion {
    fn id(&self) -> &'static str {
        "outside_region"
    }
    fn matches(&self, _: &CNode) -> bool {
        true
    }
    fn expand<'a>(&self, node: &'a CNode) -> Result<Expansion<'a>, Unsupported> {
        let mut expansion = Expansion::leaf(GraphNodeKind::Function, node);
        let draft = expansion.node.as_mut().unwrap();
        draft.span = SourceSpan { lo: 1, hi: 5 };
        let outside = SourceSpan { lo: 0, hi: 1 };
        match self.0 {
            0 => draft.parts.push(SyntaxPart {
                role: CNodeRole::Condition,
                span: outside,
            }),
            1 => draft.exits.push(ControlExit {
                kind: ControlExitKind::Return,
                span: outside,
            }),
            _ => expansion.children.push(ChildRegion::Sequence(vec![node])),
        }
        Ok(expansion)
    }
}

#[test]
fn source_parts_exits_and_children_cannot_escape_their_own_region() {
    let source = b"int f(void) { return 0; }";
    let parsed = Parser::default().parse(source);
    for mode in 0..3 {
        let result = parsed.ast.build_graph(source, vec![OutsideRegion(mode)]);
        assert!(result.is_err(), "mode {mode} escaped its region");
    }
}
