use core::ops::ControlFlow;
use std::collections::BTreeSet;

use crate::ast::*;

use firefly_diagnostics::*;
use firefly_pass::Pass;
use firefly_syntax_base::FunctionName;

use crate::visit::{self as visit, VisitMut};

/// This pass expands partially-resolved function names to their fully-qualified
/// names, when those names refer to imported functions. It does not expand calls
/// to local functions, nor does it examine unresolved calls, as we deal with those
/// much later during compilation after constant propagation and with proper dataflow
/// analysis in place to allow us to better resolve the operands of function applications.
///
/// Once this pass has run, we don't need to concern ourselves with imports anymore, as
/// the distinction is erased.
pub struct ExpandUnqualifiedCalls<'m> {
    module: &'m Module,
    locals: &'m BTreeSet<FunctionName>,
}
impl<'m> Pass for ExpandUnqualifiedCalls<'m> {
    type Input<'a> = &'a mut Function;
    type Output<'a> = &'a mut Function;

    fn run<'a>(&mut self, f: Self::Input<'a>) -> anyhow::Result<Self::Output<'a>> {
        match self.visit_mut_function(f) {
            ControlFlow::Continue(_) => Ok(f),
            ControlFlow::Break(err) => Err(err),
        }
    }
}
impl<'m> ExpandUnqualifiedCalls<'m> {
    pub fn new(module: &'m Module, locals: &'m BTreeSet<FunctionName>) -> Self {
        Self { module, locals }
    }
}
impl<'m> VisitMut<anyhow::Error> for ExpandUnqualifiedCalls<'m> {
    // Need to expand apply with an atom callee
    fn visit_mut_apply(&mut self, apply: &mut Apply) -> ControlFlow<anyhow::Error> {
        visit::visit_mut_apply(self, apply)?;
        let arity: u8 = apply.args.len().try_into().unwrap();
        let span = apply.callee.span();
        let name = match apply.callee.as_mut() {
            Expr::Literal(Literal::Atom(ident)) => {
                let local = FunctionName::new_local(ident.name, arity);
                if self.locals.contains(&local.to_local()) {
                    FunctionVar::PartiallyResolved(Span::new(span, local))
                } else if self.module.imports.contains_key(&local.to_local()) {
                    let resolved = self.module.imports.get(&local).unwrap();
                    FunctionVar::Resolved(Span::new(span, local.resolve(resolved.module)))
                } else {
                    // This is an unresolvable function, but we catch that elsewhere
                    return ControlFlow::Continue(());
                }
            }
            Expr::Remote(remote) => {
                if let Ok(name) = remote.try_eval(arity) {
                    FunctionVar::Resolved(Span::new(span, name))
                } else {
                    return ControlFlow::Continue(());
                }
            }
            _ => return ControlFlow::Continue(()),
        };

        apply.callee = Box::new(Expr::FunctionVar(name));

        ControlFlow::Continue(())
    }

    fn visit_mut_function_var(&mut self, name: &mut FunctionVar) -> ControlFlow<anyhow::Error> {
        // We only care about partially-resolved function names at this point
        if let Some(ref local) = name.partial_resolution() {
            if self.locals.contains(&local.to_local()) {
                let span = local.span();
                let function = local.function;
                let arity = local.arity;
                *name = FunctionVar::PartiallyResolved(Span::new(
                    span,
                    FunctionName::new_local(function, arity),
                ));
            } else if self.module.imports.contains_key(&local.to_local()) {
                let span = local.span();
                let function = local.function;
                let arity = local.arity;
                let resolved = self.module.imports.get(&local.to_local()).unwrap();
                *name = FunctionVar::Resolved(Span::new(
                    span,
                    FunctionName::new(resolved.module, function, arity),
                ));
            }
        }

        ControlFlow::Continue(())
    }
}

#[cfg(test)]
mod stable_local_resolution_tests {
    use super::*;
    use firefly_intern::{Ident, Symbol};
    use firefly_syntax_base::bifs;

    fn module() -> Module {
        Module::new(Ident::new(Symbol::intern("resolution_test"), SourceSpan::UNKNOWN), SourceSpan::UNKNOWN)
    }
    fn atom(name: &str) -> Expr {
        Expr::Literal(Literal::Atom(Ident::new(Symbol::intern(name), SourceSpan::UNKNOWN)))
    }

    #[test]
    fn complete_locals_override_imports_even_when_definitions_are_removed() {
        let mut module = module();
        let signature = bifs::all().iter().find(|sig| sig.mfa().function == Symbol::intern("length") && sig.mfa().arity == 1).unwrap().clone();
        let local = signature.mfa().to_local();
        module.imports.insert(local.clone(), Span::new(SourceSpan::UNKNOWN, signature));
        let locals = BTreeSet::from([local.clone(), FunctionName::new_local(Symbol::intern("earlier"), 1), FunctionName::new_local(Symbol::intern("later"), 1)]);
        assert!(module.functions.is_empty());
        let mut pass = ExpandUnqualifiedCalls::new(&module, &locals);
        for name in ["length", "earlier", "later"] {
            let mut call = Apply::new(SourceSpan::UNKNOWN, atom(name), vec![atom("arg")]);
            assert!(pass.visit_mut_apply(&mut call).is_continue());
            let Expr::FunctionVar(FunctionVar::PartiallyResolved(resolved)) = *call.callee else { panic!("local call lost identity") };
            assert_eq!(*resolved, FunctionName::new_local(Symbol::intern(name), 1));
        }
        let mut reference = FunctionVar::PartiallyResolved(Span::new(SourceSpan::UNKNOWN, FunctionName::new(module.name(), Symbol::intern("length"), 1)));
        assert!(pass.visit_mut_function_var(&mut reference).is_continue());
        assert_eq!(reference.partial_resolution().unwrap().to_local(), local);
        assert!(matches!(reference, FunctionVar::PartiallyResolved(_)));
    }

    #[test]
    fn explicit_import_and_explicit_remote_call_keep_their_modules() {
        let mut module = module();
        let local = FunctionName::new_local(Symbol::intern("length"), 1);
        let signature = bifs::all().iter().find(|sig| sig.mfa().to_local() == local).unwrap().clone();
        module.imports.insert(local.clone(), Span::new(SourceSpan::UNKNOWN, signature));
        let locals = BTreeSet::new();
        let mut pass = ExpandUnqualifiedCalls::new(&module, &locals);
        let mut call = Apply::new(SourceSpan::UNKNOWN, atom("length"), vec![atom("arg")]);
        assert!(pass.visit_mut_apply(&mut call).is_continue());
        let Expr::FunctionVar(FunctionVar::Resolved(resolved)) = *call.callee else { panic!("import unresolved") };
        assert_eq!(resolved.module, Some(Symbol::intern("erlang")));
        let mut call = Apply::new(SourceSpan::UNKNOWN, Expr::Remote(Remote::new(SourceSpan::UNKNOWN, atom("lists"), atom("reverse"))), vec![atom("arg")]);
        assert!(pass.visit_mut_apply(&mut call).is_continue());
        let Expr::FunctionVar(FunctionVar::Resolved(resolved)) = *call.callee else { panic!("remote unresolved") };
        assert_eq!(resolved.module, Some(Symbol::intern("lists")));
    }

    #[test]
    fn absent_auto_import_is_not_reintroduced() {
        let module = module();
        let locals = BTreeSet::new();
        let mut pass = ExpandUnqualifiedCalls::new(&module, &locals);
        let mut call = Apply::new(SourceSpan::UNKNOWN, atom("length"), vec![atom("arg")]);
        assert!(pass.visit_mut_apply(&mut call).is_continue());
        assert!(matches!(*call.callee, Expr::Literal(_)));
    }
}
