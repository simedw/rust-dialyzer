use std::{path::PathBuf, sync::Arc, collections::BTreeMap};
use firefly_beam::{AbstractCode, ast::Form};
use firefly_diagnostics::CodeMap;
use firefly_util::diagnostics::{CaptureEmitter, DiagnosticsConfig, DiagnosticsHandler};
use firefly_syntax_erl::passes::{AbstractErlangToAst, SemanticAnalysis, CanonicalizeSyntax, AstToCore};
use firefly_syntax_base::ApplicationMetadata;
use firefly_intern::Symbol;
use firefly_pass::Pass;

fn maximum_debug_field(debug: &str, field: &str) -> usize {
 debug.split(field).skip(1).filter_map(|part| part.split(|c: char| !c.is_ascii_digit()).next()?.parse::<usize>().ok()).max().unwrap_or(1)
}
fn lower(path: &str, out: &std::path::Path) -> anyhow::Result<()> {
 let forms = AbstractCode::from_beam_file(path)?;
 let debug = format!("{forms:?}");
 let lines = maximum_debug_field(&debug, "line: ") + 2;
 let columns = maximum_debug_field(&debug, "column: ") + 2;
 anyhow::ensure!(lines.checked_mul(columns + 1).unwrap_or(usize::MAX) <= 16 * 1024 * 1024, "synthetic source too large");
 let synthetic = format!("{}\n", " ".repeat(columns)).repeat(lines);
 let codemap = Arc::new(CodeMap::new());
 for form in &forms.forms {
  if let Form::File(file) = form {
   codemap.add(PathBuf::from(file.original_file.as_str().get()), synthetic.clone());
  }
 }
 let emitter = Arc::new(CaptureEmitter::new());
 let diagnostics = Arc::new(DiagnosticsHandler::new(DiagnosticsConfig::default(), codemap.clone(), emitter.clone()));
 let mut ast = AbstractErlangToAst::new(&diagnostics, &codemap).run(forms.into())?;
 let types_before = ast.types.len();
 let specs_before = ast.specs.len();
 let app = ApplicationMetadata { name: Symbol::intern("audit"), modules: BTreeMap::new() };
 ast = SemanticAnalysis::new(&diagnostics, &app).run(ast)?;
 ast = CanonicalizeSyntax::new(codemap).run(ast)?;
 let core = AstToCore::new(diagnostics.clone()).run(ast)?;
 anyhow::ensure!(!diagnostics.has_errors(), "compiler diagnostics: {}", emitter.captured());
 let filename = std::path::Path::new(path).file_stem().unwrap().to_string_lossy();
 std::fs::write(out.join(format!("{filename}.core")), core.to_string())?;
 std::fs::write(out.join(format!("{filename}.core.debug")), format!("{core:#?}"))?;
 println!("LOWER OK path={path} functions={} types_before={types_before} specs_before={specs_before} diagnostics={:?}", core.functions.len(), emitter.captured());
 Ok(())
}
fn main() {
 let args: Vec<String> = std::env::args().skip(1).collect();
 let out = PathBuf::from(&args[0]);
 std::fs::create_dir_all(&out).unwrap();
 let mut failed = 0;
 for path in &args[1..] {
  match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| lower(path, &out))) {
   Ok(Ok(())) => (),
   Ok(Err(e)) => { eprintln!("LOWER ERROR path={path} error={e:#}"); failed += 1; },
   Err(_) => { eprintln!("LOWER PANIC path={path}"); failed += 1; }
  }
 }
 std::process::exit(if failed == 0 {0} else {1});
}
