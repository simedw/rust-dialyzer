use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use firefly_beam::{AbstractCode, ast::Form};
use firefly_diagnostics::CodeMap;
use firefly_intern::{Symbol, symbols};
use firefly_pass::Pass;
use firefly_syntax_base::{ApplicationMetadata, FunctionName};
use firefly_syntax_core::{Expr, Module, Try, PrimOp};
use firefly_syntax_erl::passes::{AbstractErlangToAst, AstToCore, CanonicalizeSyntax, SemanticAnalysis};
use firefly_util::diagnostics::{CaptureEmitter, DiagnosticsConfig, DiagnosticsHandler};

fn fixture() -> Module {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/rethrow/firefly_rethrow_identity.beam");
    let forms = AbstractCode::from_beam_file(path).unwrap();
    let codemap = Arc::new(CodeMap::new());
    for form in &forms.forms {
        if let Form::File(file) = form {
            codemap.add(PathBuf::from(file.original_file.as_str().get()), format!("{}\n", " ".repeat(128)).repeat(32));
        }
    }
    let emitter = Arc::new(CaptureEmitter::new());
    let diagnostics = Arc::new(DiagnosticsHandler::new(DiagnosticsConfig::default(), codemap.clone(), emitter.clone()));
    let ast = AbstractErlangToAst::new(&diagnostics, &codemap).run(forms.into()).unwrap();
    let app = ApplicationMetadata { name: Symbol::intern("audit"), modules: BTreeMap::new() };
    let ast = SemanticAnalysis::new(&diagnostics, &app).run(ast).unwrap();
    let ast = CanonicalizeSyntax::new(codemap).run(ast).unwrap();
    let core = AstToCore::new(diagnostics.clone()).run(ast).unwrap();
    assert!(!diagnostics.has_errors(), "{}", emitter.captured());
    core
}
fn normal(expr: &Expr) -> &Expr {
    match expr {
        Expr::Case(case) => normal(&case.clauses[0].body),
        Expr::Let(let_expr) => normal(&let_expr.body),
        other => other,
    }
}
fn body<'a>(core: &'a Module, name: &str, arity: u8) -> &'a Expr {
    normal(&core.functions[&FunctionName::new_local(Symbol::intern(name), arity)].fun.body)
}
fn rethrow(try_expr: &Try) -> &PrimOp {
    let Expr::Case(handler) = try_expr.handler.as_ref() else { panic!("expected catch handler") };
    let Expr::PrimOp(op) = normal(&handler.clauses.last().unwrap().body) else { panic!("expected generated raise") };
    op
}
#[test]
fn catch_and_after_rethrows_preserve_class_reason_and_info_order() {
    let core = fixture();
    for name in ["catch_rethrow", "after_rethrow"] {
        let Expr::Try(try_expr) = body(&core, name, 1) else { panic!("expected Try for {name}") };
        let op = rethrow(try_expr);
        assert_eq!(op.name, symbols::Raise);
        assert_eq!(op.args.len(), 3);
        assert!(op.annotations.contains(Symbol::intern("generated_rethrow")));
        assert!(op.annotations.contains(symbols::CompilerGenerated));
        for (arg, expected) in op.args.iter().zip(&try_expr.evars) {
            let Expr::Var(var) = arg else { panic!("rethrow operands must be exception vars") };
            assert_eq!(var.name, expected.name, "{name} must keep [class, reason, info] ordering");
        }
    }
}
#[test]
fn explicit_raise_keeps_bif_operands_and_has_no_rethrow_marker() {
    let core = fixture();
    let Expr::PrimOp(op) = body(&core, "explicit_raise", 0) else { panic!("expected explicit raise") };
    assert_eq!(op.name, symbols::Raise);
    assert_eq!(op.args.len(), 3);
    assert_eq!(op.args[0].as_atom(), Some(symbols::Error));
    assert_eq!(op.args[1].as_atom(), Some(Symbol::intern("reason")));
    assert!(!op.annotations.contains(Symbol::intern("generated_rethrow")));
    assert!(!op.annotations.contains(symbols::CompilerGenerated));
}
