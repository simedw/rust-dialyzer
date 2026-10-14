use crate::bifs::registry::{Mfa, Registry};
use crate::core::{CoreExpr, CoreModule, ErlangTerm, NodeId};
use crate::lattice::Type;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReturnKind {
    NoMatch,
    OnlyExplicit,
    OnlyNormal,
    Both,
}
fn literal(module: &CoreModule, node: NodeId) -> Option<&ErlangTerm> {
    match module.nodes[node.0].expr {
        CoreExpr::Literal(term) => Some(&module.terms[term.0]),
        _ => None,
    }
}
fn folds_to_literal(module: &CoreModule, node: NodeId) -> bool {
    let mut pending = vec![node];
    while let Some(node) = pending.pop() {
        match &module.nodes[node.0].expr {
            CoreExpr::Literal(_) => {}
            CoreExpr::Tuple(elements) => pending.extend(elements),
            CoreExpr::Cons { head, tail } => {
                pending.push(*head);
                pending.push(*tail);
            }
            _ => return false,
        }
    }
    true
}
fn annotated(module: &CoreModule, node: NodeId, tag: &str) -> bool {
    let mut term = module.nodes[node.0].annotations;
    loop {
        match &module.terms[term.0] {
            ErlangTerm::List { items, tail } => {
                if items
                    .iter()
                    .any(|t| matches!(&module.terms[t.0], ErlangTerm::Atom(value) if value == tag))
                {
                    return true;
                }
                term = *tail;
            }
            _ => return false,
        }
    }
}
pub fn classify(
    module: &CoreModule,
    function: NodeId,
    bifs: &Registry,
) -> Result<ReturnKind, String> {
    let CoreExpr::Fun { body, .. } = module.nodes[function.0].expr else {
        return Err("return classification requires function".into());
    };
    let mut pending = vec![body];
    let (mut explicit, mut normal) = (false, false);
    while let Some(node) = pending.pop() {
        match &module.nodes[node.0].expr {
            CoreExpr::Call {
                module: m,
                name: f,
                args,
            } => match (literal(module, *m), literal(module, *f)) {
                (Some(ErlangTerm::Atom(m)), Some(ErlangTerm::Atom(f))) => {
                    let mfa = Mfa {
                        module: m.clone().into(),
                        function: f.clone().into(),
                        arity: args.len(),
                    };
                    if bifs.is_known(&mfa)
                        && bifs
                            .transfer(&mfa, &vec![Type::any(); args.len()])
                            .map_err(|e| e.to_string())?
                            .result
                            .is_none()
                    {
                        explicit = true;
                    } else {
                        normal = true;
                    }
                }
                _ if folds_to_literal(module, *m) && folds_to_literal(module, *f) => normal = true,
                _ => {
                    explicit = true;
                    normal = true;
                }
            },
            CoreExpr::Case { clauses, .. } => {
                if clauses.is_empty() {
                    normal |= annotated(module, node, "receive_timeout");
                } else {
                    pending.extend(clauses);
                }
            }
            CoreExpr::Receive {
                clauses,
                timeout,
                action,
            } => {
                if matches!(literal(module, *timeout), Some(ErlangTerm::Atom(v)) if v == "infinity")
                {
                    if clauses.is_empty() {
                        normal = true;
                    } else {
                        pending.extend(clauses);
                    }
                } else {
                    pending.extend(clauses);
                    pending.push(*action);
                }
            }
            CoreExpr::Try(try_payload) => {
                let crate::core::TryExpr { body, handler, .. } = try_payload.as_ref();

                pending.push(*handler);
                pending.push(*body);
            }
            CoreExpr::Catch { body }
            | CoreExpr::Let { body, .. }
            | CoreExpr::Letrec { body, .. }
            | CoreExpr::Clause { body, .. }
            | CoreExpr::Seq { body, .. } => pending.push(*body),
            CoreExpr::Primop { .. } => {}
            CoreExpr::Apply { .. }
            | CoreExpr::Binary(_)
            | CoreExpr::Bitstr { .. }
            | CoreExpr::Cons { .. }
            | CoreExpr::Fun { .. }
            | CoreExpr::Literal(_)
            | CoreExpr::Map { .. }
            | CoreExpr::Tuple(_)
            | CoreExpr::Values(_)
            | CoreExpr::Var(_) => normal = true,
            other => {
                return Err(format!(
                    "unsupported return-classification expression {}",
                    other.kind()
                ));
            }
        }
        if explicit && normal {
            break;
        }
    }
    Ok(match (explicit, normal) {
        (false, false) => ReturnKind::NoMatch,
        (true, false) => ReturnKind::OnlyExplicit,
        (false, true) => ReturnKind::OnlyNormal,
        (true, true) => ReturnKind::Both,
    })
}
