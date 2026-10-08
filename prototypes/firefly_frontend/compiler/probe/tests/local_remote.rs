use std::{collections::BTreeMap, path::PathBuf, sync::Arc};
use firefly_beam::{AbstractCode, ast::Form};
use firefly_diagnostics::CodeMap;
use firefly_intern::{Symbol, symbols};
use firefly_pass::Pass;
use firefly_syntax_base::{ApplicationMetadata, FunctionName};
use firefly_syntax_core::{Expr, Module};
use firefly_syntax_erl::passes::{AbstractErlangToAst, AstToCore, CanonicalizeSyntax, SemanticAnalysis};
use firefly_util::diagnostics::{CaptureEmitter, DiagnosticsConfig, DiagnosticsHandler};

fn fixture() -> Module {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../fixtures/local_remote/firefly_call_identity.beam");
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
fn body<'a>(core: &'a Module, name: &str) -> &'a Expr {
    normal(&core.functions[&FunctionName::new_local(Symbol::intern(name), 0)].fun.body)
}
fn normal(expr: &Expr) -> &Expr {
    match expr {
        Expr::Case(case) => normal(&case.clauses[0].body),
        other => other,
    }
}
fn assert_local_apply(expr: &Expr) {
    let Expr::Apply(apply) = expr else { panic!("expected local Apply, got {expr:?}") };
    let Expr::Var(var) = apply.callee.as_ref() else { panic!("expected function variable") };
    assert_eq!(var.name.name, Symbol::intern("hidden"));
    assert_eq!(var.arity, Some(1));
}
#[test]
fn local_and_explicit_remote_calls_keep_distinct_identity() {
    let core = fixture();
    assert!(!core.exports.iter().any(|f| f.function == Symbol::intern("hidden")));
    assert_local_apply(body(&core, "local"));
    let Expr::Call(call) = body(&core, "remote") else { panic!("explicit self-module call must remain remote") };
    assert_eq!(call.module.as_atom(), Some(Symbol::intern("firefly_call_identity")));
    assert_eq!(call.function.as_atom(), Some(Symbol::intern("hidden")));
}
#[test]
fn captures_and_nested_closures_keep_local_identity() {
    let core = fixture();
    let Expr::Var(local) = body(&core, "local_capture") else { panic!("local capture must be function variable") };
    assert_eq!(local.name.name, Symbol::intern("hidden"));
    assert_eq!(local.arity, Some(1));
    let Expr::PrimOp(remote) = body(&core, "remote_capture") else { panic!("remote capture must stay make_fun") };
    assert_eq!(remote.name, symbols::MakeFun);
    assert_eq!(remote.args[0].as_atom(), Some(Symbol::intern("firefly_call_identity")));
    let Expr::Fun(fun) = body(&core, "closure") else { panic!("expected nested closure") };
    assert_local_apply(normal(&fun.body));
}
