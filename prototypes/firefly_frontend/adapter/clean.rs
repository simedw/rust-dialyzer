// SPDX-License-Identifier: Apache-2.0
// Core cleaning semantics derived from OTP dialyzer_clean_core.erl.
use firefly_diagnostics::SourceSpan;
use firefly_intern::{Symbol, symbols};
use firefly_syntax_base::{Annotated, Annotations, Literal, Var};
use firefly_syntax_core::{Case, Clause, Expr, Module, PrimOp, Tuple};

pub(super) fn clean(module: &mut Module) -> Result<(), String> {
    for function in module.functions.values_mut() {
        clean_expr(&mut function.fun.body, &mut vec![])?;
    }
    Ok(())
}

fn ignored(expr: &Expr) -> bool {
    expr.annotations()
        .contains(Symbol::intern("dialyzer_ignore"))
}
fn transfer(expr: &Expr, gotos: &[Var]) -> bool {
    if ignored(expr) {
        return true;
    }
    match expr {
        Expr::Apply(apply) => match apply.callee.as_ref() {
            Expr::Var(var) => gotos.iter().any(|target| target.name() == var.name()),
            _ => false,
        },
        _ => false,
    }
}
fn raising(expr: &Expr) -> bool {
    matches!(expr, Expr::PrimOp(op) if op.name.as_str().get() == "match_fail" || op.name.as_str().get() == "raise")
}
fn ignored_clause(clause: &Clause, gotos: &[Var]) -> bool {
    if clause
        .annotations
        .contains(Symbol::intern("dialyzer_ignore"))
        || raising(&clause.body)
    {
        return true;
    }
    if transfer(&clause.body, gotos) {
        return true;
    }
    clause.is_compiler_generated()
        && matches!(clause.body.as_ref(), Expr::Seq(seq)
        if matches!(seq.arg.as_ref(), Expr::PrimOp(op) if op.name == symbols::RecvNext && op.args.is_empty())
        && transfer(&seq.body, gotos))
}
fn clauses(clauses: &mut Vec<Clause>, gotos: &mut Vec<Var>) -> Result<(), String> {
    clauses.retain(|clause| !ignored_clause(clause, gotos));
    for clause in clauses {
        if let Some(guard) = &mut clause.guard {
            clean_expr(guard, gotos)?;
        }
        clean_expr(&mut clause.body, gotos)?;
    }
    Ok(())
}
fn atom(span: SourceSpan, name: &str) -> Expr {
    Expr::Literal(Literal::atom(span, Symbol::intern(name)))
}
fn clean_expr(expr: &mut Expr, gotos: &mut Vec<Var>) -> Result<(), String> {
    if let Expr::LetRec(rec) = expr {
        if rec.annotations.contains(symbols::LetrecGoto) {
            if rec.defs.len() != 1 {
                return Err("letrec_goto requires exactly one function".into());
            }
            let (name, def) = &rec.defs[0];
            let Expr::Fun(fun) = def else {
                return Err("letrec_goto definition is not a fun".into());
            };
            let mut fun = fun.clone();
            let mut body = rec.body.as_ref().clone();
            let direct = transfer(&body, &[name.clone()]);
            gotos.push(name.clone());
            clean_expr(&mut fun.body, gotos)?;
            clean_expr(&mut body, gotos)?;
            gotos.pop();
            *expr = if direct {
                *fun.body
            } else {
                let span = SourceSpan::default();
                let unknown = Expr::PrimOp(PrimOp::new(
                    span,
                    Symbol::intern("dialyzer_unknown"),
                    vec![],
                ));
                let first = Clause::new(span, vec![atom(span, "a")], body);
                let mut pattern = vec![atom(span, "b")];
                pattern.extend(fun.vars.into_iter().map(Expr::Var));
                let second = Clause::new(
                    span,
                    vec![Expr::Tuple(Tuple::new(span, pattern))],
                    *fun.body,
                );
                Expr::Case(Case {
                    span,
                    annotations: Annotations::default(),
                    arg: Box::new(unknown),
                    clauses: vec![first, second],
                })
            };
            return Ok(());
        }
    }
    if let Expr::If(branch) = expr {
        let skip_then = transfer(&branch.then_body, gotos) || raising(&branch.then_body);
        let skip_else = transfer(&branch.else_body, gotos) || raising(&branch.else_body);
        if skip_then || skip_else {
            let span = branch.span;
            clean_expr(&mut branch.guard, gotos)?;
            let mut alternatives = vec![];
            if !skip_then {
                clean_expr(&mut branch.then_body, gotos)?;
                alternatives.push(Clause::new(
                    span,
                    vec![atom(span, "true")],
                    branch.then_body.as_ref().clone(),
                ));
            }
            if !skip_else {
                clean_expr(&mut branch.else_body, gotos)?;
                alternatives.push(Clause::new(
                    span,
                    vec![atom(span, "false")],
                    branch.else_body.as_ref().clone(),
                ));
            }
            *expr = Expr::Case(Case {
                span,
                annotations: branch.annotations.clone(),
                arg: branch.guard.clone(),
                clauses: alternatives,
            });
            return Ok(());
        }
    }
    match expr {
        Expr::Alias(_) | Expr::Literal(_) | Expr::Var(_) => {}
        Expr::Apply(x) => {
            clean_expr(&mut x.callee, gotos)?;
            list(&mut x.args, gotos)?;
        }
        Expr::Binary(x) => {
            for segment in &mut x.segments {
                clean_expr(&mut segment.value, gotos)?;
                if let Some(size) = &mut segment.size {
                    clean_expr(size, gotos)?;
                }
            }
        }
        Expr::Call(x) => {
            list(&mut x.args, gotos)?;
            clean_expr(&mut x.module, gotos)?;
            clean_expr(&mut x.function, gotos)?;
        }
        Expr::Case(x) => {
            clean_expr(&mut x.arg, gotos)?;
            clauses(&mut x.clauses, gotos)?;
        }
        Expr::Catch(x) => clean_expr(&mut x.body, gotos)?,
        Expr::Cons(x) => {
            clean_expr(&mut x.head, gotos)?;
            clean_expr(&mut x.tail, gotos)?;
        }
        Expr::Fun(x) => clean_expr(&mut x.body, gotos)?,
        Expr::If(x) => {
            clean_expr(&mut x.guard, gotos)?;
            clean_expr(&mut x.then_body, gotos)?;
            clean_expr(&mut x.else_body, gotos)?;
        }
        Expr::Let(x) => {
            clean_expr(&mut x.arg, gotos)?;
            clean_expr(&mut x.body, gotos)?;
        }
        Expr::LetRec(x) => {
            for (name, def) in &mut x.defs {
                clean_expr(def, gotos)?;
                normalize_definition(name, def)?;
            }
            clean_expr(&mut x.body, gotos)?;
        }
        Expr::Map(x) => {
            clean_expr(&mut x.arg, gotos)?;
            for pair in &mut x.pairs {
                clean_expr(&mut pair.key, gotos)?;
                clean_expr(&mut pair.value, gotos)?;
            }
        }
        Expr::PrimOp(x) => list(&mut x.args, gotos)?,
        Expr::Receive(x) => {
            clauses(&mut x.clauses, gotos)?;
            clean_expr(&mut x.timeout, gotos)?;
            clean_expr(&mut x.action, gotos)?;
        }
        Expr::Seq(x) => {
            clean_expr(&mut x.arg, gotos)?;
            clean_expr(&mut x.body, gotos)?;
        }
        Expr::Try(x) => {
            clean_expr(&mut x.arg, gotos)?;
            clean_expr(&mut x.body, gotos)?;
            clean_expr(&mut x.handler, gotos)?;
        }
        Expr::Tuple(x) => list(&mut x.elements, gotos)?,
        Expr::Values(x) => list(&mut x.values, gotos)?,
    }
    Ok(())
}
fn normalize_definition(name: &Var, definition: &mut Expr) -> Result<(), String> {
    loop {
        let Expr::LetRec(inner) = definition else {
            break;
        };
        if inner.defs.len() != 1 {
            break;
        }
        let (inner_name, function) = &inner.defs[0];
        if inner_name != name
            || !matches!(inner.body.as_ref(), Expr::Var(returned) if returned == inner_name)
            || !matches!(function, Expr::Fun(_))
        {
            break;
        }
        *definition = function.clone();
    }
    if matches!(definition, Expr::Fun(_)) {
        Ok(())
    } else {
        Err(format!(
            "letrec definition {} must be a fun after named-function normalization",
            name.name()
        ))
    }
}
fn list(exprs: &mut [Expr], gotos: &mut Vec<Var>) -> Result<(), String> {
    for expr in exprs {
        clean_expr(expr, gotos)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use firefly_intern::Ident;
    use firefly_syntax_core::{Apply, Fun, If, LetRec, Seq};
    fn span() -> SourceSpan {
        SourceSpan::UNKNOWN
    }
    fn variable() -> Var {
        Var::new_with_arity(Ident::with_empty_span(Symbol::intern("loop")), 0)
    }
    fn apply(var: &Var) -> Expr {
        Expr::Apply(Apply {
            span: span(),
            annotations: Annotations::default(),
            callee: Box::new(Expr::Var(var.clone())),
            args: vec![],
        })
    }
    fn fun(body: Expr) -> Expr {
        Expr::Fun(Fun {
            span: span(),
            annotations: Annotations::default(),
            name: Symbol::intern("loop"),
            vars: vec![],
            body: Box::new(body),
        })
    }
    fn goto(body: Expr, entry: Expr) -> Expr {
        Expr::LetRec(LetRec {
            span: span(),
            annotations: Annotations::from([symbols::LetrecGoto]),
            defs: vec![(variable(), fun(body))],
            body: Box::new(entry),
        })
    }
    #[test]
    fn named_function_definition_keeps_recursive_fun_and_outer_binding() {
        let name = variable();
        let recursive = fun(apply(&name));
        let inner = Expr::LetRec(LetRec {
            span: span(),
            annotations: Annotations::default(),
            defs: vec![(name.clone(), recursive.clone())],
            body: Box::new(Expr::Var(name.clone())),
        });
        let mut outer = Expr::LetRec(LetRec {
            span: span(),
            annotations: Annotations::default(),
            defs: vec![(name.clone(), inner)],
            body: Box::new(apply(&name)),
        });
        clean_expr(&mut outer, &mut vec![]).unwrap();
        let Expr::LetRec(rec) = outer else {
            panic!("outer recursive scope lost");
        };
        assert_eq!(rec.defs[0].0, name);
        assert_eq!(rec.defs[0].1, recursive);
        assert_eq!(*rec.body, apply(&name));
    }
    #[test]
    fn escaped_named_function_scope_is_not_stripped() {
        let name = variable();
        let mut value = Expr::LetRec(LetRec {
            span: span(),
            annotations: Annotations::default(),
            defs: vec![(name.clone(), fun(apply(&name)))],
            body: Box::new(Expr::Var(name)),
        });
        let original = value.clone();
        clean_expr(&mut value, &mut vec![]).unwrap();
        assert_eq!(value, original);
    }
    #[test]
    fn different_binder_definition_requires_explicit_semantic_conversion() {
        let name = variable();
        let other = Var::new_with_arity(Ident::with_empty_span(Symbol::intern("different")), 0);
        let mut definition = Expr::LetRec(LetRec {
            span: span(),
            annotations: Annotations::default(),
            defs: vec![(other.clone(), fun(apply(&other)))],
            body: Box::new(Expr::Var(other)),
        });
        let original = definition.clone();
        assert!(normalize_definition(&name, &mut definition).is_err());
        assert_eq!(definition, original);
    }
    #[test]
    fn receive_goto_drops_only_compiler_fallback_and_loop_transfer() {
        let var = variable();
        let mut fallback = Clause::new(
            span(),
            vec![atom(span(), "other")],
            Expr::Seq(Seq::new(
                span(),
                Expr::PrimOp(PrimOp::new(span(), symbols::RecvNext, vec![])),
                apply(&var),
            )),
        );
        fallback.mark_compiler_generated();
        let real = Clause::new(span(), vec![atom(span(), "message")], atom(span(), "ok"));
        let case = Expr::Case(Case {
            span: span(),
            annotations: Annotations::default(),
            arg: Box::new(atom(span(), "unknown")),
            clauses: vec![real, fallback],
        });
        let body = Expr::If(If::new(span(), atom(span(), "true"), case, apply(&var)));
        let mut expr = goto(body, apply(&var));
        clean_expr(&mut expr, &mut vec![]).unwrap();
        let Expr::Case(outer) = expr else {
            panic!("transfer branch must become filtered case");
        };
        assert_eq!(outer.clauses.len(), 1);
        let Expr::Case(inner) = outer.clauses[0].body.as_ref() else {
            panic!("message case missing");
        };
        assert_eq!(inner.clauses.len(), 1);
        assert_eq!(*inner.clauses[0].body, atom(span(), "ok"));
    }
    #[test]
    fn ordinary_letrec_and_unmarked_recv_next_are_preserved() {
        let var = variable();
        let body = Expr::PrimOp(PrimOp::new(span(), symbols::RecvNext, vec![]));
        let mut expr = Expr::LetRec(LetRec {
            span: span(),
            annotations: Annotations::default(),
            defs: vec![(var.clone(), fun(body))],
            body: Box::new(apply(&var)),
        });
        let original = expr.clone();
        clean_expr(&mut expr, &mut vec![]).unwrap();
        assert_eq!(expr, original);
    }
    #[test]
    fn pattern_goto_preserves_both_nontransfer_paths_under_unknown() {
        let mut expr = goto(atom(span(), "fallback"), atom(span(), "first"));
        clean_expr(&mut expr, &mut vec![]).unwrap();
        let Expr::Case(case) = expr else {
            panic!("pattern goto alternatives missing");
        };
        assert!(
            matches!(case.arg.as_ref(), Expr::PrimOp(op) if op.name.as_str().get() == "dialyzer_unknown")
        );
        assert_eq!(case.clauses.len(), 2);
        assert_eq!(*case.clauses[0].body, atom(span(), "first"));
        assert_eq!(*case.clauses[1].body, atom(span(), "fallback"));
        assert!(matches!(case.clauses[1].patterns[0], Expr::Tuple(_)));
    }
    #[test]
    fn raising_and_explicit_ignore_clauses_are_removed() {
        let raise = Clause::new(
            span(),
            vec![atom(span(), "bad")],
            Expr::PrimOp(PrimOp::new(span(), Symbol::intern("match_fail"), vec![])),
        );
        let mut ignore = Clause::new(
            span(),
            vec![atom(span(), "ignored")],
            atom(span(), "hidden"),
        );
        ignore.annotations.set(Symbol::intern("dialyzer_ignore"));
        let good = Clause::new(span(), vec![atom(span(), "ok")], atom(span(), "value"));
        let mut values = vec![raise, ignore, good];
        clauses(&mut values, &mut vec![]).unwrap();
        assert_eq!(values.len(), 1);
    }
}
