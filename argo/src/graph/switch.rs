use super::{ChildRegion, Unsupported, cases};
use crate::{CNode, CNodeRole as Role, CNodeType as Kind};

enum Item<'a> {
    Label(&'a CNode),
    Statement(&'a CNode),
}

pub(super) fn groups(node: &CNode) -> Result<Vec<ChildRegion<'_>>, Unsupported> {
    let body = node
        .children
        .iter()
        .find(|node| node.role == Some(Role::Body))
        .unwrap();
    if body.node_type != Kind::Block {
        return Err(Unsupported("switch body is not a compound scope"));
    }
    let mut items = Vec::new();
    for statement in &body.children {
        flatten(statement, &mut items)?;
    }
    let (mut labels, mut statements): (Vec<&CNode>, Vec<&CNode>) = (Vec::new(), Vec::new());
    let mut groups = Vec::new();
    for item in items {
        match item {
            Item::Label(label) => {
                if statements
                    .last()
                    .is_some_and(|node| cases::terminates(node))
                {
                    groups.push(ChildRegion::CaseGroup {
                        labels: std::mem::take(&mut labels),
                        statements: std::mem::take(&mut statements),
                    });
                }
                labels.push(label);
            }
            Item::Statement(statement) => {
                if labels.is_empty() {
                    return Err(Unsupported("statements precede the first case label"));
                }
                statements.push(statement);
            }
        }
    }
    if !labels.is_empty() {
        groups.push(ChildRegion::CaseGroup { labels, statements });
    }
    Ok(groups)
}

fn flatten<'a>(node: &'a CNode, items: &mut Vec<Item<'a>>) -> Result<(), Unsupported> {
    if matches!(node.node_type, Kind::Case | Kind::Default) {
        items.push(Item::Label(node));
        if let Some(body) = node
            .children
            .iter()
            .find(|child| child.role == Some(Role::Body))
        {
            flatten(body, items)?;
        }
    } else {
        if nested_label(node) {
            return Err(Unsupported(
                "case label is nested in another control or scope",
            ));
        }
        items.push(Item::Statement(node));
    }
    Ok(())
}

fn nested_label(node: &CNode) -> bool {
    if node.node_type == Kind::Switch {
        return false;
    }
    matches!(node.node_type, Kind::Case | Kind::Default) || node.children.iter().any(nested_label)
}
