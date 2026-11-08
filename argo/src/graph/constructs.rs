use super::{ChildRegion, Expansion, GraphNodeKind as Kind, SyntaxPart, Unsupported};
use super::{regions, sequence, switch};
use crate::{CNode, CNodeRole as Role, CNodeType};

pub(crate) fn function(node: &CNode) -> Result<Expansion<'_>, Unsupported> {
    require(node, &[Role::Signature, Role::Body])?;
    if node
        .children
        .iter()
        .any(|child| child.role == Some(Role::Signature) && child.is_variadic)
    {
        return Err(Unsupported("variadic bodies are opaque in catalog v1"));
    }
    control(node, Kind::Function)
}

pub(crate) fn branch(node: &CNode) -> Result<Expansion<'_>, Unsupported> {
    require(node, &[Role::Condition, Role::Then])?;
    control(node, Kind::Branch)
}

pub(crate) fn loop_region(node: &CNode) -> Result<Expansion<'_>, Unsupported> {
    require(node, &[Role::Body])?;
    if node.node_type != CNodeType::For {
        require(node, &[Role::Condition])?;
    }
    control(node, Kind::Loop)
}

pub(crate) fn else_region(node: &CNode) -> Result<Expansion<'_>, Unsupported> {
    require(node, &[Role::Body])?;
    let mut expansion = control(node, Kind::Scope)?;
    expansion.node.as_mut().unwrap().name = Some("else".into());
    Ok(expansion)
}

pub(crate) fn block(node: &CNode) -> Result<Expansion<'_>, Unsupported> {
    let mut expansion = Expansion::leaf(Kind::Scope, node);
    expansion.children = regions::partition(&node.children);
    Ok(expansion)
}

pub(crate) fn straight_line(node: &CNode) -> Result<Expansion<'_>, Unsupported> {
    sequence::expand(&[node]).map_err(|_| Unsupported("empty sequence"))
}

pub(crate) fn switch_region(node: &CNode) -> Result<Expansion<'_>, Unsupported> {
    require(node, &[Role::Selector, Role::Body])?;
    let mut expansion = control(node, Kind::Switch)?;
    expansion.children = switch::groups(node)?;
    Ok(expansion)
}

fn require(node: &CNode, roles: &[Role]) -> Result<(), Unsupported> {
    if roles
        .iter()
        .all(|role| node.children.iter().any(|child| child.role == Some(*role)))
    {
        Ok(())
    } else {
        Err(Unsupported("required structural child roles are missing"))
    }
}

fn control(node: &CNode, kind: Kind) -> Result<Expansion<'_>, Unsupported> {
    let mut expansion = Expansion::leaf(kind, node);
    for child in &node.children {
        if matches!(child.role, Some(Role::Body | Role::Then | Role::Else)) {
            expansion.children.push(ChildRegion::Syntax(child));
        } else if let Some(role) = child.role {
            if child.walk().any(|node| {
                matches!(
                    node.node_type,
                    CNodeType::Opaque | CNodeType::Block | CNodeType::Goto | CNodeType::Label
                )
            }) {
                return Err(Unsupported("unsupported syntax in a control header"));
            }
            expansion.node.as_mut().unwrap().parts.push(SyntaxPart {
                role,
                span: child.span.into(),
            });
        }
    }
    Ok(expansion)
}
