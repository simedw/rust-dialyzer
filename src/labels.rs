use crate::{
    core::{CoreExpr, CoreModule, CoreProgram, ErlangTerm, NodeId, TermId},
    program::{NodeRef, VariableName},
};
use std::collections::VecDeque;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CoreLabel(pub u64);

#[derive(Debug)]
pub struct Labels {
    pub nodes: Vec<Vec<Option<CoreLabel>>>,
    pub next_label: CoreLabel,
    reannotated: Vec<Vec<bool>>,
}
type Environment = im::OrdMap<VariableName, CoreLabel>;
enum Action {
    Walk(NodeId, Environment),
    Assign(NodeId),
    BindAndWalk {
        vars: Vec<NodeId>,
        body: NodeId,
        environment: Environment,
    },
}
fn name(module: &CoreModule, node: NodeId) -> Result<VariableName, String> {
    let CoreExpr::Var(id) = module.nodes[node.0].expr else {
        return Err("label binder is not a variable".into());
    };
    match &module.terms[id.0] {
        ErlangTerm::Atom(name) => Ok(VariableName::Atom(name.into())),
        ErlangTerm::Integer(value) => Ok(VariableName::Integer(value.into())),
        ErlangTerm::Tuple(values) if values.len() == 2 => {
            let ErlangTerm::Atom(function) = &module.terms[values[0].0] else {
                return Err("function variable name is not an atom".into());
            };
            let ErlangTerm::Integer(arity) = &module.terms[values[1].0] else {
                return Err("function variable arity is not an integer".into());
            };
            Ok(VariableName::Function(
                function.into(),
                arity
                    .parse()
                    .map_err(|_| "function variable arity exceeds platform range")?,
            ))
        }
        _ => Err("invalid Core variable name".into()),
    }
}
fn inherited(module: &CoreModule, mut annotation: TermId) -> Result<Option<CoreLabel>, String> {
    loop {
        match &module.terms[annotation.0] {
            ErlangTerm::Nil => return Ok(None),
            ErlangTerm::List { items, tail } => {
                for item in items {
                    if let ErlangTerm::Tuple(pair) = &module.terms[item.0]
                        && pair.len() == 2
                        && matches!(&module.terms[pair[0].0],ErlangTerm::Atom(tag) if tag=="label")
                    {
                        let ErlangTerm::Integer(value) = &module.terms[pair[1].0] else {
                            return Err("Core label annotation is not an integer".into());
                        };
                        return value
                            .parse()
                            .map(CoreLabel)
                            .map(Some)
                            .map_err(|_| "Core label annotation exceeds supported range".into());
                    }
                }
                annotation = *tail;
            }
            _ => return Err("Core annotations are not a proper list".into()),
        }
    }
}
fn allocate(next: &mut u64) -> Result<CoreLabel, String> {
    let label = CoreLabel(*next);
    *next = next.checked_add(1).ok_or("Core label counter overflow")?;
    Ok(label)
}
fn bind(
    module: &CoreModule,
    vars: &[NodeId],
    mut environment: Environment,
    labels: &mut [Option<CoreLabel>],
    reannotated: &mut [bool],
    next: &mut u64,
) -> Result<Environment, String> {
    for node in vars {
        let label = allocate(next)?;
        environment.insert(name(module, *node)?, label);
        labels[node.0] = Some(label);
        reannotated[node.0] = true;
    }
    Ok(environment)
}
fn pattern_variables(module: &CoreModule, patterns: &[NodeId]) -> Result<Vec<NodeId>, String> {
    let mut todo: Vec<_> = patterns.iter().rev().copied().collect();
    let mut variables = VecDeque::new();
    while let Some(node) = todo.pop() {
        match &module.nodes[node.0].expr {
            CoreExpr::Var(_) => variables.push_front(node),
            CoreExpr::Literal(_) => {}
            CoreExpr::Cons { head, tail } => {
                todo.push(*head);
                todo.push(*tail);
            }
            CoreExpr::Tuple(nodes) | CoreExpr::Binary(nodes) => todo.extend(nodes.iter().rev()),
            CoreExpr::Map { pairs, .. } => todo.extend(pairs.iter().rev()),
            #[cfg(otp_domain = "29")]
            CoreExpr::Record { pairs, .. } => todo.extend(pairs.iter().rev()),
            #[cfg(otp_domain = "29")]
            CoreExpr::RecordPair { value, .. } => todo.push(*value),
            CoreExpr::MapPair {
                operation, value, ..
            } => {
                todo.push(*value);
                todo.push(*operation);
            }
            CoreExpr::Bitstr { value, .. } => todo.push(*value),
            CoreExpr::Alias { variable, pattern } => {
                variables.push_front(*variable);
                todo.push(*pattern);
            }
            _ => {
                return Err(format!(
                    "invalid Core clause pattern {}",
                    module.nodes[node.0].expr.kind()
                ));
            }
        }
    }
    Ok(variables.into())
}
impl Labels {
    pub fn label(&self, node: NodeRef) -> Option<CoreLabel> {
        self.nodes[node.module.0][node.node]
    }
    pub fn is_reannotated(&self, node: NodeRef) -> bool {
        self.reannotated[node.module.0][node.node]
    }
    /// Labels are allocated from one counter in module order. Each module
    /// is walked independently from zero and then shifted by the number of
    /// labels allocated in the modules before it, which gives the same labels.
    pub fn build(core: &CoreProgram, initial: CoreLabel) -> Result<Self, String> {
        let walked = crate::ordered_parallel::setup_map(&core.modules, module_labels);
        let mut next = initial.0;
        let mut modules = Vec::with_capacity(walked.len());
        let mut changed_modules = Vec::with_capacity(walked.len());
        for result in walked {
            let (mut labels, reannotated, allocated) = result?;
            if next != 0 {
                for (label, changed) in labels.iter_mut().zip(&reannotated) {
                    if *changed && let Some(label) = label {
                        label.0 += next;
                    }
                }
            }
            next = next
                .checked_add(allocated)
                .ok_or("Core label counter overflow")?;
            modules.push(labels);
            changed_modules.push(reannotated);
        }
        Ok(Self {
            nodes: modules,
            reannotated: changed_modules,
            next_label: CoreLabel(next),
        })
    }
}

/// Labels one module with a counter starting at zero. Every label it
/// allocates is marked reannotated; the rest are inherited annotations.
fn module_labels(module: &CoreModule) -> Result<(Vec<Option<CoreLabel>>, Vec<bool>, u64), String> {
    let mut next = 0;
    let mut labels = module
        .nodes
        .iter()
        .map(|node| inherited(module, node.annotations))
        .collect::<Result<Vec<_>, _>>()?;
    let mut reannotated = vec![false; module.nodes.len()];
    let mut todo = vec![Action::Walk(module.core, Environment::new())];
    while let Some(action) = todo.pop() {
        match action {
            Action::Assign(node) => {
                labels[node.0] = Some(allocate(&mut next)?);
                reannotated[node.0] = true;
            }
            Action::BindAndWalk {
                vars,
                body,
                environment,
            } => {
                let environment = bind(
                    module,
                    &vars,
                    environment,
                    &mut labels,
                    &mut reannotated,
                    &mut next,
                )?;
                todo.push(Action::Walk(body, environment));
            }
            Action::Walk(node, environment) => {
                let expression = &module.nodes[node.0].expr;
                if matches!(expression, CoreExpr::Literal(_) | CoreExpr::Opaque(_)) {
                    continue;
                }
                if matches!(expression, CoreExpr::Var(_)) {
                    reannotated[node.0] = true;
                    labels[node.0] = Some(match environment.get(&name(module, node)?) {
                        Some(label) => *label,
                        None => allocate(&mut next)?,
                    });
                    continue;
                }
                todo.push(Action::Assign(node));
                let mut children = Vec::new();
                match expression {
                    CoreExpr::Values(nodes) | CoreExpr::Tuple(nodes) | CoreExpr::Binary(nodes) => {
                        children.extend(nodes)
                    }
                    CoreExpr::Cons { head, tail } => children.extend([*head, *tail]),
                    #[cfg(otp_domain = "29")]
                    CoreExpr::Record { argument, id, pairs } => { children.extend([*argument, *id]); children.extend(pairs); }
                    #[cfg(otp_domain = "29")]
                    CoreExpr::RecordPair { key, value } => children.extend([*key, *value]),
                    CoreExpr::Map {
                        base,
                        pairs,
                        pattern,
                    } => {
                        if !pattern {
                            children.push(*base);
                        }
                        children.extend(pairs);
                    }
                    CoreExpr::MapPair {
                        operation,
                        key,
                        value,
                    } => children.extend([*operation, *key, *value]),
                    CoreExpr::Let {
                        vars,
                        argument,
                        body,
                    } => {
                        todo.push(Action::BindAndWalk {
                            vars: vars.clone(),
                            body: *body,
                            environment: environment.clone(),
                        });
                        todo.push(Action::Walk(*argument, environment));
                        continue;
                    }
                    CoreExpr::Seq { argument, body } => children.extend([*argument, *body]),
                    CoreExpr::Apply { operator, args } => {
                        children.push(*operator);
                        children.extend(args);
                    }
                    CoreExpr::Call { module, name, args } => {
                        children.extend([*module, *name]);
                        children.extend(args);
                    }
                    CoreExpr::Primop { name, args } => {
                        children.push(*name);
                        children.extend(args);
                    }
                    CoreExpr::Case { argument, clauses } => {
                        children.push(*argument);
                        children.extend(clauses);
                    }
                    CoreExpr::Clause {
                        patterns,
                        guard,
                        body,
                    } => {
                        let variables = pattern_variables(module, patterns)?;
                        let scoped = bind(
                            module,
                            &variables,
                            environment,
                            &mut labels,
                            &mut reannotated,
                            &mut next,
                        )?;
                        let children = patterns
                            .iter()
                            .copied()
                            .chain([*guard, *body])
                            .collect::<Vec<_>>();
                        todo.extend(
                            children
                                .into_iter()
                                .rev()
                                .map(|node| Action::Walk(node, scoped.clone())),
                        );
                        continue;
                    }
                    CoreExpr::Alias { variable, pattern } => children.extend([*variable, *pattern]),
                    CoreExpr::Fun { params, body } => {
                        let scoped = bind(
                            module,
                            params,
                            environment,
                            &mut labels,
                            &mut reannotated,
                            &mut next,
                        )?;
                        todo.push(Action::Walk(*body, scoped));
                        continue;
                    }
                    CoreExpr::Receive {
                        clauses,
                        timeout,
                        action,
                    } => {
                        children.extend(clauses);
                        children.extend([*timeout, *action]);
                    }
                    CoreExpr::Try(try_payload) => {
                        let crate::core::TryExpr {
                            argument,
                            vars,
                            body,
                            exception_vars,
                            handler,
                        } = try_payload.as_ref();

                        todo.push(Action::BindAndWalk {
                            vars: exception_vars.clone(),
                            body: *handler,
                            environment: environment.clone(),
                        });
                        todo.push(Action::BindAndWalk {
                            vars: vars.clone(),
                            body: *body,
                            environment: environment.clone(),
                        });
                        todo.push(Action::Walk(*argument, environment));
                        continue;
                    }
                    CoreExpr::Catch { body } => children.push(*body),
                    CoreExpr::Bitstr {
                        value,
                        size,
                        unit,
                        typ,
                        flags,
                    } => children.extend([*value, *size, *unit, *typ, *flags]),
                    CoreExpr::Letrec { definitions, body } => {
                        let vars = definitions.iter().map(|pair| pair.0).collect::<Vec<_>>();
                        let scoped = bind(
                            module,
                            &vars,
                            environment,
                            &mut labels,
                            &mut reannotated,
                            &mut next,
                        )?;
                        let children = definitions
                            .iter()
                            .flat_map(|(key, value)| [*key, *value])
                            .chain([*body])
                            .collect::<Vec<_>>();
                        todo.extend(
                            children
                                .into_iter()
                                .rev()
                                .map(|node| Action::Walk(node, scoped.clone())),
                        );
                        continue;
                    }
                    CoreExpr::Module(module_payload) => {
                        let crate::core::ModuleExpr {
                            definitions,
                            attributes,
                            exports,
                            ..
                        } = module_payload.as_ref();

                        let vars = definitions.iter().map(|pair| pair.0).collect::<Vec<_>>();
                        let scoped = bind(
                            module,
                            &vars,
                            environment,
                            &mut labels,
                            &mut reannotated,
                            &mut next,
                        )?;
                        let children = attributes
                            .iter()
                            .chain(definitions)
                            .flat_map(|(key, value)| [*key, *value])
                            .chain(exports.iter().copied())
                            .collect::<Vec<_>>();
                        todo.extend(
                            children
                                .into_iter()
                                .rev()
                                .map(|node| Action::Walk(node, scoped.clone())),
                        );
                        continue;
                    }
                    CoreExpr::Literal(_) | CoreExpr::Var(_) | CoreExpr::Opaque(_) => {
                        unreachable!()
                    }
                }
                todo.extend(
                    children
                        .into_iter()
                        .rev()
                        .map(|node| Action::Walk(node, environment.clone())),
                );
            }
        }
    }
    Ok((labels, reannotated, next))
}
