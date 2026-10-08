use rust_dialyzer_poc::{
    core::ErlangTerms,
    core::{ErlangTerm, TermId},
    form_cache_wire::cache_to_value,
    form_context::{FormContext, FormOperation},
    form_exports::var_table_new,
    lattice::{Type, VariableId},
    native_form_environment::{NativeFormSite, NativeSiteKind, RawFormTerm},
    program::TypeName,
    resolver::FormCache,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead},
    sync::Arc,
};
fn run(v: &Value) -> Result<Value, String> {
    let exported = Arc::new(ErlangTerms::parse(&v["exports"])?);
    let contexts = v["snapshots"]
        .as_array()
        .ok_or("snapshots")?
        .iter()
        .map(|a| FormContext::from_type_tables(Arc::new(ErlangTerms::parse(a)?), exported.clone()))
        .collect::<Result<Vec<_>, _>>()?;
    let mut cache = FormCache::new();
    let mut results = Vec::new();
    for step in v["steps"].as_array().ok_or("steps")? {
        if step["fresh"] == true {
            cache = FormCache::new();
        }
        if step["clear_module_cache"] == true {
            let mut a = ErlangTerms::parse(&cache_to_value(&cache)?)?;
            let ErlangTerm::Tuple(fields) = &a.terms[a.root.0] else {
                return Err("native cache tuple".into());
            };
            let modules = fields[2];
            a.terms[modules.0] = ErlangTerm::Map(Vec::new());
            cache = FormCache::from_native_arena(Arc::new(a))?;
        }
        let mut vars = var_table_new();
        if let Some(variables) = step["variables"].as_array() {
            for variable in variables {
                let name = match variable["name"]["kind"].as_str() {
                    Some("atom") => VariableId::Atom(
                        variable["name"]["value"]
                            .as_str()
                            .ok_or("variable atom")?
                            .into(),
                    ),
                    Some("integer") => VariableId::Integer(
                        variable["name"]["value"]
                            .as_str()
                            .ok_or("variable integer")?
                            .parse()
                            .map_err(|_| "integer")?,
                    ),
                    _ => return Err("unsupported variable name".into()),
                };
                vars.insert(name, Type::from_value(&variable["type"])?);
            }
        }
        let kind = match step["site"]["kind"].as_str().unwrap_or("spec") {
            "spec" => NativeSiteKind::Spec,
            "record" => NativeSiteKind::Record,
            "check" => NativeSiteKind::Check,
            "type" => NativeSiteKind::Type,
            _ => return Err("invalid typed site".into()),
        };
        let file = if !step["site"]["file"].is_null() {
            Arc::new(ErlangTerms::parse(&step["site"]["file"])?)
        } else {
            Arc::new(ErlangTerms {
                root: TermId(0),
                terms: vec![ErlangTerm::Binary(b"cache.erl".to_vec())],
            })
        };
        let site = NativeFormSite {
            kind,
            scope: TypeName {
                module: step["site"]["module"]
                    .as_str()
                    .or(step["module"].as_str())
                    .unwrap_or("m")
                    .into(),
                name: step["site"]["name"].as_str().unwrap_or("probe").into(),
                arity: step["site"]["arity"].as_u64().unwrap_or(0) as usize,
            }
            .into(),
            file: RawFormTerm::new(file.clone(), file.root)?,
        };
        let operation = match step["mode"].as_str().unwrap_or("resolve") {
            "resolve" => FormOperation::Resolve(vars),
            "without_remote" => FormOperation::WithoutRemote,
            "check_remote" => FormOperation::CheckRemote,
            "check_fields" => FormOperation::CheckRecordFields(vars),
            _ => return Err("unknown export operation".into()),
        };
        let context = &contexts[step["snapshot"].as_u64().ok_or("snapshot")? as usize];
        let form = Arc::new(ErlangTerms::parse(&step["form"])?);
        let form = RawFormTerm::new(form.clone(), form.root)?;
        match context.execute(
            &site,
            &form,
            &cache,
            operation,
        ) {
            Ok(outcome) => {
                cache = FormCache::from_native_arena(Arc::new(ErlangTerms::parse(
                    &cache_to_value(&outcome.cache)?,
                )?))?;
                results.push(json!({"ok":outcome.value.unwrap_or_else(||Type::atom("ok")).to_native_value().to_value(),"cache":cache_to_value(&cache)?,"events":outcome.external_types.iter().map(|e|{let a=e.to_arena();rust_dialyzer_poc::type_exports::arena_to_value(&a.arena().terms,a.id())}).collect::<Vec<_>>()}));
            }
            Err(error) => results.push(json!({"error":error.error.report(),"cache":cache_to_value(&cache)?,"events":error.external_types.iter().map(|e|{let a=e.to_arena();rust_dialyzer_poc::type_exports::arena_to_value(&a.arena().terms,a.id())}).collect::<Vec<_>>()})),
        }
    }
    Ok(json!(results))
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .and_then(|v| run(&v));
        println!(
            "{}",
            match result {
                Ok(v) => json!({"ok":v}),
                Err(e) => json!({"error":e}),
            }
        );
    }
}
