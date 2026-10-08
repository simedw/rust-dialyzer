use crate::{
    contract_runtime::ContractSummary,
    forms::{Contract, FormId, TypeForm},
    lattice::{LatticeError, Type, TypeView, VariableId, VariableView},
    resolver::{Resolver, Site},
};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, crate::form_error::FormError>;
fn algebra(e: LatticeError) -> crate::form_error::FormError {
    e.into()
}

fn vars(resolver: &Resolver<'_>, id: FormId, out: &mut Vec<VariableId>) -> Result<()> {
    use crate::core::ErlangTerm;
    let raw = resolver
        .raw_form(id)
        .ok_or("contract variable scan lacks source arena")?;
    let mut work = vec![raw.id()];
    while let Some(id) = work.pop() {
        match &raw.arena().terms[id.0] {
            ErlangTerm::Tuple(parts) => {
                if let [tag, _, name] = parts.as_slice()
                    && matches!(&raw.arena().terms[tag.0],ErlangTerm::Atom(s) if s=="var")
                {
                    match &raw.arena().terms[name.0] {
                        ErlangTerm::Atom(s) if s != "_" => {
                            out.push(VariableId::Atom(s.clone()));
                        }
                        ErlangTerm::Integer(n) => {
                            out.push(VariableId::Integer(
                                n.parse().map_err(|_| "variable integer")?,
                            ));
                        }
                        _ => {}
                    }
                }
                work.extend(parts);
            }
            ErlangTerm::List { items, tail } => {
                work.extend(items);
                work.push(*tail);
            }
            _ => {}
        }
    }
    Ok(())
}

// digraph uses an ETS bag (insertion order), then collect_elems reverses it.
// Its one_path retains visited vertices across backtracking and prunes length-one
// cycles before falling back to a self-loop. Keep the search iterative.
fn cycle_first(
    start: &VariableId,
    edges: &BTreeSet<(VariableId, VariableId)>,
    insertion_order: &[(VariableId, VariableId)],
) -> Option<VariableId> {
    let neighbours = |v: &VariableId| {
        insertion_order
            .iter()
            .rev()
            .filter(|(from, to)| from == v && edges.contains(&(from.clone(), to.clone())))
            .map(|(_, to)| to.clone())
            .collect::<Vec<_>>()
    };
    let mut visited = BTreeSet::from([start.clone()]);
    let mut stack = vec![(neighbours(start), 0usize)];
    let mut path = vec![start.clone()];
    while let Some((adjacent, cursor)) = stack.last_mut() {
        if *cursor == adjacent.len() {
            stack.pop();
            path.pop();
            continue;
        }
        let next = adjacent[*cursor].clone();
        *cursor += 1;
        if &next == start {
            if path.len() >= 2 {
                return Some(path[1].clone());
            }
        } else if visited.insert(next.clone()) {
            path.push(next.clone());
            stack.push((neighbours(&next), 0));
        }
    }
    edges
        .contains(&(start.clone(), start.clone()))
        .then(|| start.clone())
}
pub fn compile(resolver: &mut Resolver<'_>, contract: &Contract) -> Result<ContractSummary> {
    compile_at(resolver, contract, None)
}
pub(crate) fn compile_at(
    resolver: &mut Resolver<'_>,
    contract: &Contract,
    origin: Option<&crate::program::SourceOrigin>,
) -> Result<ContractSummary> {
    let mut alternatives = Vec::new();
    for id in &contract.alternatives {
        resolver.materialize(*id)?;
        let (function, ids) = match &resolver.forms.nodes[id.0].value {
            TypeForm::BoundedFunction {
                function,
                constraints,
            } => (*function, constraints.clone()),
            _ => (*id, Vec::new()),
        };
        let mut initialized = Vec::new();
        let mut constraints = Vec::new();
        let mut edges = BTreeSet::new();
        let mut insertion_order = Vec::new();
        for id in ids {
            if resolver.materialize(id).is_err() {
                let raw = resolver
                    .raw_form(id)
                    .ok_or("constraint lacks source arena")?;
                let mut b = crate::form_error::ReasonBuilder::new();
                let value = b.copy(&raw);
                let tag = b.atom("case_clause");
                let root = b.tuple(vec![tag, value]);
                return Err(crate::form_error::FormError::ErrorReason(b.finish(root)));
            }
            let TypeForm::Constraint { name, arguments } = resolver.forms.nodes[id.0].value.clone()
            else {
                return Err("constraint grammar".into());
            };
            if name != "is_subtype" || arguments.len() != 2 {
                let mut b = crate::form_error::ReasonBuilder::new();
                let guard = b.atom(&name);
                let arity = b.integer(arguments.len());
                let root = b.tuple(vec![guard, arity]);
                let raw = b.finish(root);
                let args = [
                    crate::native_form_environment::RawFormTerm::new(raw.arena().clone(), guard)?,
                    crate::native_form_environment::RawFormTerm::new(raw.arena().clone(), arity)?,
                ];
                let message = crate::native_form_io::format(
                    "Unsupported type guard ~tw/~w\n",
                    &args,
                    &BTreeMap::new(),
                )?;
                let mut b = crate::form_error::ReasonBuilder::new();
                let message = b.copy(&message);
                let error = b.atom("error");
                let root = b.tuple(vec![error, message]);
                return Err(crate::form_error::FormError::Throw(b.finish(root)));
            }
            // Native initialization resolves the LHS before validating its variable identity.
            resolver.check_record_fields(arguments[0], &BTreeMap::new())?;
            let left = resolver.resolve(arguments[0], Site::Spec, BTreeMap::new())?;
            initialized.push((left, arguments[1]));
        }
        // initialize_constraints resolves every LHS first and prepends entries;
        // find_uses then validates variable identities in that reversed order.
        for (left, rhs) in initialized.iter().rev() {
            let name = match left.view() {
                TypeView::Variable(VariableView::Atom(name)) => VariableId::Atom(name.into()),
                TypeView::Variable(VariableView::Integer(name)) => {
                    VariableId::Integer(name.clone())
                }
                _ => {
                    return Err(crate::form_error::FormError::Native(
                        crate::native_error::NativeTransferError::FunctionClause,
                    ));
                }
            };
            let mut used = Vec::new();
            vars(resolver, *rhs, &mut used)?;
            for use_name in used {
                let edge = (name.clone(), use_name);
                if edges.insert(edge.clone()) {
                    insertion_order.push(edge);
                }
            }
            constraints.push((name.clone(), *rhs));
        }
        let original = edges.clone();
        let vertices = edges
            .iter()
            .flat_map(|(a, b)| [a.clone(), b.clone()])
            .collect::<BTreeSet<_>>();
        loop {
            let count = edges.len();
            for v in &vertices {
                if let Some(next) = cycle_first(v, &edges, &insertion_order) {
                    edges.remove(&(v.clone(), next));
                }
            }
            if edges.len() == count {
                break;
            }
        }
        let removed = original
            .difference(&edges)
            .cloned()
            .collect::<BTreeSet<_>>();
        let mut table = BTreeMap::new();
        loop {
            let mut next: BTreeMap<VariableId, Type> = BTreeMap::new();
            for (name, id) in &constraints {
                let mut variables = table.clone();
                for (from, to) in &removed {
                    if from == name {
                        variables.insert(to.clone(), Type::any());
                    }
                }
                resolver.check_record_fields(*id, &variables)?;
                let value = resolver.resolve(*id, Site::Spec, variables)?;
                let value = if let Some(previous) = next.get(name) {
                    previous.meet(&value).map_err(algebra)?
                } else {
                    value
                };
                next.insert(name.clone(), value);
            }
            if table == next {
                break;
            }
            table = next;
        }
        let ordinary = !matches!(
            resolver.forms.nodes[id.0].value,
            TypeForm::BoundedFunction { .. }
        );
        let wrap = |e: crate::form_error::FormError| {
            if ordinary {
                match origin {
                    Some(source) => e.at_position(source),
                    None => e,
                }
            } else {
                e
            }
        };
        resolver
            .check_record_fields(function, &table)
            .map_err(wrap)?;
        let signature = resolver
            .resolve_contract(
                function,
                contract.metadata,
                table.clone(),
                contract.alternatives.len() == 1 && !contract.callback,
            )
            .map_err(wrap)?
            .substitute(&BTreeMap::new())
            .map_err(algebra)?;
        alternatives.push((
            signature,
            table
                .into_iter()
                .map(|(name, value)| {
                    (
                        match name {
                            VariableId::Atom(v) => Type::variable_atom(v),
                            VariableId::Integer(v) => Type::variable_id(VariableId::Integer(v)),
                        },
                        value,
                    )
                })
                .collect(),
        ));
    }
    ContractSummary::from_parts(alternatives)
        .map_err(|e| crate::form_error::FormError::Transport(e.to_string()))
}
