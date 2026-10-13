//! Check a compiled Mix project.
//!
//! Usage: rust-dialyzer [PROJECT] [--env ENV] [--app APP]... [--include-app APP]...
//!
//! Analyzes the project's apps with everything they depend on, read from
//! `_build/ENV/lib`, Elixir and OTP, without a PLT, and prints the warnings for
//! the project's own modules. OTP and Elixir are located through `erl` and
//! `elixir` on PATH. Exits 0 without warnings, 2 with warnings, 1 on error.

use rust_dialyzer_poc::{
    beam_frontend::pipeline::{self, ContextSource, Emission},
    core::{ErlangTerm, ErlangTerms},
    warning_render::{FilenameOption, LocationOption, Options, Renderer},
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    process::{Command, exit},
    time::Instant,
};

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

const USAGE: &str =
    "usage: rust-dialyzer [PROJECT] [--env ENV] [--app APP]... [--include-app APP]...";

// Dialyzer's default warnings plus warn_unknown, as dialyxir enables them.
const WARNINGS: [&str; 15] = [
    "warn_behaviour",
    "warn_bin_construction",
    "warn_callgraph",
    "warn_contract_range",
    "warn_contract_syntax",
    "warn_contract_types",
    "warn_failing_call",
    "warn_fun_app",
    "warn_map_construction",
    "warn_matching",
    "warn_non_proper_list",
    "warn_not_called",
    "warn_return_no_exit",
    "warn_undefined_callbacks",
    "warn_unknown",
];

// No PLT: every module in scope is analyzed from its BEAM file.
const EMPTY_PRIOR: &str = r#"{"version":5,
 "profile":{"otp":"28","elixir":"1.20.4","erts":"16.2","dialyzer":"5.4",
  "term_map_order":"native_to_list_with_iterator",
  "core_options":["no_copt","to_core","binary","return_errors","no_inline",
   "strict_record_tests","dialyzer","no_spawn_compiler_process"]},
 "modules":[],"source":{"kind":"classic_plt","path":"","sha256":"","files":[]},
 "contracts":[],"summaries":[],
 "raw_callback_groups":{"root":0,"terms":[{"kind":"nil"}]},
 "type_tables":{"root":0,"terms":[{"kind":"map","entries":[],"iterator_order":[]}]},
 "exported_types":{"root":0,"terms":[{"kind":"nil"}]},
 "raw_summaries":{"root":0,"terms":[{"kind":"nil"}]},
 "raw_contracts":{"root":0,"terms":[{"kind":"nil"}]}}"#;

struct Args {
    project: PathBuf,
    env: String,
    apps: Vec<String>,
    include: Vec<String>,
}

fn fail(message: impl std::fmt::Display) -> ! {
    eprintln!("rust-dialyzer: {message}");
    exit(1)
}

fn parse_args() -> Args {
    let mut args = Args {
        project: PathBuf::from("."),
        env: "dev".into(),
        apps: vec![],
        include: vec![],
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut value = || it.next().unwrap_or_else(|| fail(USAGE));
        match arg.as_str() {
            "--env" => args.env = value(),
            "--app" => args.apps.push(value()),
            "--include-app" => args.include.push(value()),
            "-h" | "--help" => {
                println!("{USAGE}");
                exit(0)
            }
            _ if arg.starts_with('-') => fail(USAGE),
            _ => args.project = PathBuf::from(arg),
        }
    }
    args
}

fn ask(program: &str, args: &[&str]) -> PathBuf {
    let output = Command::new(program)
        .args(args)
        .output()
        .unwrap_or_else(|e| fail(format!("cannot run {program}: {e}; is it on PATH?")));
    if !output.status.success() {
        fail(format!("{program} failed: {}", String::from_utf8_lossy(&output.stderr)));
    }
    PathBuf::from(String::from_utf8_lossy(&output.stdout).trim())
}

/// The list under `{key, [...]}` in an `.app` file.
fn app_list(text: &str, key: &str) -> Vec<String> {
    let Some(start) = text.find(&format!("{{{key},")) else {
        return vec![];
    };
    let rest = &text[start..];
    let (Some(open), Some(close)) = (rest.find('['), rest.find(']')) else {
        return vec![];
    };
    rest[open + 1..close]
        .split(',')
        .map(|name| name.trim().trim_matches('\'').to_owned())
        .filter(|name| !name.is_empty())
        .collect()
}

/// App name to its ebin directory and required apps; later roots win.
fn app_index(roots: &[PathBuf]) -> BTreeMap<String, (PathBuf, Vec<String>)> {
    let mut apps = BTreeMap::new();
    for root in roots {
        let Ok(dirs) = std::fs::read_dir(root) else { continue };
        let mut dirs: Vec<_> = dirs.flatten().map(|d| d.path().join("ebin")).collect();
        dirs.sort();
        for ebin in dirs {
            let Ok(files) = std::fs::read_dir(&ebin) else { continue };
            for file in files.flatten().map(|f| f.path()) {
                if file.extension().is_none_or(|e| e != "app") {
                    continue;
                }
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                let optional = app_list(&text, "optional_applications");
                let deps = app_list(&text, "applications")
                    .into_iter()
                    .chain(app_list(&text, "included_applications"))
                    .filter(|d| !optional.contains(d))
                    .collect();
                let name = file.file_stem().unwrap().to_string_lossy().into_owned();
                apps.insert(name, (ebin.clone(), deps));
            }
        }
    }
    apps
}

/// App names declared by `app: :name` in mix.exs, including umbrella children.
fn project_apps(project: &Path) -> Vec<String> {
    let mut files = vec![project.join("mix.exs")];
    if let Ok(children) = std::fs::read_dir(project.join("apps")) {
        let mut children: Vec<_> = children.flatten().map(|c| c.path().join("mix.exs")).collect();
        children.sort();
        files.extend(children);
    }
    files
        .iter()
        .filter_map(|f| std::fs::read_to_string(f).ok())
        .filter_map(|text| {
            let at = text.find("app: :")? + "app: :".len();
            let name: String = text[at..]
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            (!name.is_empty()).then_some(name)
        })
        .collect()
}

fn beams(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<_> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|f| f.path())
        .filter(|p| p.extension().is_some_and(|e| e == "beam"))
        .collect();
    found.sort();
    found
}

fn main() {
    let args = parse_args();
    let project = std::fs::canonicalize(&args.project)
        .unwrap_or_else(|e| fail(format!("{}: {e}", args.project.display())));
    let build = project.join("_build").join(&args.env);
    if !build.join("lib").is_dir() {
        fail(format!(
            "{}/lib not found; run `MIX_ENV={} mix compile` first",
            build.display(),
            args.env
        ));
    }
    let otp = ask("erl", &["-noshell", "-eval", "io:format(\"~s\",[code:lib_dir()]),halt()."]);
    let dialyzer = ask(
        "erl",
        &["-noshell", "-eval", "io:format(\"~s\",[code:lib_dir(dialyzer)]),halt()."],
    )
    .join("ebin");
    let elixir = ask(
        "elixir",
        &["-e", "IO.write(Path.dirname(to_string(:code.lib_dir(:elixir))))"],
    );

    let apps = app_index(&[otp, elixir, build.join("lib")]);
    let own = if args.apps.is_empty() { project_apps(&project) } else { args.apps };
    if own.is_empty() {
        fail("no `app:` found in mix.exs; pass --app");
    }
    if let Some(app) = own.iter().find(|a| !apps.contains_key(*a)) {
        fail(format!("no compiled {app}.app under {}/lib", build.display()));
    }
    let mut selected = BTreeSet::new();
    let roots = ["erts", "kernel", "stdlib", "elixir"].map(String::from);
    let mut pending: Vec<String> = own.iter().chain(&roots).chain(&args.include).cloned().collect();
    while let Some(app) = pending.pop() {
        if let Some((_, deps)) = apps.get(&app)
            && selected.insert(app)
        {
            pending.extend(deps.iter().cloned());
        }
    }
    // Protocol consolidation replaces the protocol modules compiled with the app.
    let mut by_name = BTreeMap::new();
    for path in selected.iter().flat_map(|app| beams(&apps[app].0)) {
        by_name.insert(path.file_name().unwrap().to_owned(), path);
    }
    for path in beams(&build.join("consolidated")) {
        if let Some(slot) = by_name.get_mut(path.file_name().unwrap()) {
            *slot = path;
        }
    }
    let paths: Vec<String> = by_name.values().map(|p| p.to_string_lossy().into_owned()).collect();
    let report: Vec<String> = own
        .iter()
        .flat_map(|app| beams(&apps[app].0))
        .map(|p| p.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    eprintln!(
        "analyzing {} modules from {} apps, reporting on {}",
        paths.len(),
        selected.len(),
        own.join(", ")
    );
    let read = |name: &str| {
        let path = dialyzer.join(name);
        std::fs::read(&path).unwrap_or_else(|e| fail(format!("{}: {e}", path.display())))
    };
    let (typesig, contracts) = (read("dialyzer_typesig.beam"), read("dialyzer_contracts.beam"));
    let options = json!({
        "id": project.file_name().map(|n| n.to_string_lossy().into_owned()),
        "location": "column", "legal_warnings": WARNINGS, "purged_modules": [],
        "use_contracts": true, "solvers": [], "report_modules": report,
    });

    // Guard analysis recurses deeply; give it the same stack as core_cache_analyze.
    let warnings = std::thread::Builder::new()
        .stack_size(4 << 30)
        .spawn(move || -> Result<Value, String> {
            let started = Instant::now();
            let loaded = pipeline::load_emission(
                &paths,
                ContextSource::Native {
                    typesig_beam: &typesig,
                    contracts_beam: &contracts,
                },
                Emission::Fused,
            )
            .map_err(|e| format!("{e}\n(the analyzer supports BEAM files compiled with OTP 28 and Elixir 1.19)"))?;
            let load = started.elapsed().as_secs_f64();
            rust_dialyzer_poc::teardown::skip_drops_at_exit();
            let started = Instant::now();
            let mut projected = rust_dialyzer_poc::compiled_probe::analyze_projected(
                &loaded.core,
                &options,
                EMPTY_PRIOR,
                &loaded.closure_profile,
            )?;
            eprintln!(
                "load {load:.1}s, analysis {:.1}s",
                started.elapsed().as_secs_f64()
            );
            std::mem::forget(loaded);
            std::mem::forget(projected.signatures);
            Ok(projected.result["warnings"].take())
        })
        .expect("analysis thread")
        .join()
        .unwrap_or_else(|_| fail("analysis panicked"))
        .unwrap_or_else(|e| fail(e));

    let arena = ErlangTerms::parse(&warnings).unwrap_or_else(|e| fail(e));
    let (mut rows, mut cell) = (vec![], arena.root);
    while let ErlangTerm::List { items, tail } = &arena.terms[cell.0] {
        rows.extend(items.iter().copied());
        cell = *tail;
    }
    let renderer = Renderer::default();
    let render = Options {
        indent: true,
        filename: FilenameOption::Fullpath,
        location: LocationOption::Column,
    };
    let prefix = format!("{}/", project.display());
    for &row in &rows {
        match renderer.format_warning(&arena, row, render) {
            Ok(text) => print!("{}", text.strip_prefix(&prefix).unwrap_or(&text)),
            Err(e) => println!("(unrenderable warning: {e})"),
        }
    }
    eprintln!("{} warnings", rows.len());
    exit(if rows.is_empty() { 0 } else { 2 })
}
