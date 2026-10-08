use crate::core::{CoreExpr, CoreModule, CoreProgram, ErlangTerm, NodeId, TermId};
use crate::program::FunctionName;
use crate::warnings::{Profile, Registry, Tag};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize)]
pub struct FunctionPolicy {
    pub function: FunctionName,
    pub profile: Profile,
    pub suppressed: bool,
}
#[derive(Clone, Debug, Serialize)]
pub struct ModulePolicy {
    pub module: String,
    pub profile: Profile,
    pub functions: Vec<FunctionPolicy>,
}
#[derive(Clone, Debug, Serialize)]
pub struct Policy {
    pub modules: Vec<ModulePolicy>,
}
#[derive(Clone, Debug, Serialize)]
pub struct PolicyError {
    pub module: String,
    pub attribute: usize,
    pub message: String,
}

fn literal(module: &CoreModule, node: NodeId) -> Result<TermId, String> {
    match module.nodes[node.0].expr {
        CoreExpr::Literal(term) | CoreExpr::Var(term) => Ok(term),
        _ => Err("attribute is not a literal".into()),
    }
}
fn atom(module: &CoreModule, term: TermId) -> Option<&str> {
    match &module.terms[term.0] {
        ErlangTerm::Atom(value) => Some(value),
        _ => None,
    }
}
fn list(module: &CoreModule, mut term: TermId) -> Result<Vec<TermId>, String> {
    let mut result = Vec::new();
    loop {
        match &module.terms[term.0] {
            ErlangTerm::Nil => return Ok(result),
            ErlangTerm::List { items, tail } => {
                result.extend(items);
                term = *tail;
            }
            _ => return Err("attribute is not a proper list".into()),
        }
    }
}
fn flatten(module: &CoreModule, term: TermId) -> Result<Vec<TermId>, String> {
    let mut result = Vec::new();
    let mut pending = vec![term];
    while let Some(term) = pending.pop() {
        match &module.terms[term.0] {
            ErlangTerm::Nil => {}
            ErlangTerm::List { .. } => pending.extend(list(module, term)?.into_iter().rev()),
            _ => result.push(term),
        }
    }
    Ok(result)
}
fn function(module: &CoreModule, term: TermId) -> Result<FunctionName, String> {
    if let ErlangTerm::Tuple(items) = &module.terms[term.0]
        && items.len() == 2
        && let Some(name) = atom(module, items[0])
        && let ErlangTerm::Integer(value) = &module.terms[items[1].0]
        && let Ok(arity) = value.parse::<usize>()
    {
        return Ok(FunctionName {
            module: module.name.clone(),
            name: name.into(),
            arity,
        });
    }
    Err("bad function in warning attribute".into())
}
fn function_options(
    module: &CoreModule,
    args: &[TermId],
    only_tag: Option<&str>,
    known: &BTreeSet<FunctionName>,
) -> Result<Vec<(FunctionName, String)>, String> {
    let mut pairs = Vec::new();
    for term in args {
        if let ErlangTerm::Tuple(items) = &module.terms[term.0]
            && items.len() == 2
        {
            for tag in flatten(module, items[0])? {
                if let Some(tag) = atom(module, tag)
                    && only_tag.is_none_or(|required| required == tag)
                {
                    for term in flatten(module, items[1])? {
                        pairs.push((term, tag.to_string()));
                    }
                }
            }
        }
    }
    let pairs = pairs
        .into_iter()
        .map(|(term, tag)| Ok((function(module, term)?, tag)))
        .collect::<Result<Vec<_>, String>>()?;
    for (function, _) in &pairs {
        if !known.contains(function) {
            return Err(format!(
                "unknown function {}/{} in warning attribute",
                function.name, function.arity
            ));
        }
    }
    Ok(pairs)
}

impl Policy {
    pub fn build(
        core: &CoreProgram,
        registry: &Registry,
        initial: &Profile,
    ) -> Result<Self, PolicyError> {
        let mut modules = Vec::new();
        for module in &core.modules {
            let CoreExpr::Module {
                attributes,
                definitions,
                ..
            } = &module.nodes[module.core.0].expr
            else {
                unreachable!()
            };
            let mut attribute = module.core.0;
            let result = (|| -> Result<ModulePolicy, String> {
                let known = definitions
                    .iter()
                    .map(|(key, _)| function(module, literal(module, *key)?))
                    .collect::<Result<BTreeSet<_>, _>>()?;
                let mut profile = initial.clone();
                let mut function_rules: BTreeMap<FunctionName, BTreeSet<String>> = BTreeMap::new();
                let mut compile_options = Vec::new();
                let mut unused = Vec::new();
                for (key, value) in attributes {
                    attribute = key.0;
                    let tag = atom(module, literal(module, *key)?);
                    if matches!(tag, Some("compile" | "dialyzer")) {
                        let args = list(module, literal(module, *value)?)?;
                        if tag == Some("compile") {
                            compile_options.extend(
                                args.iter()
                                    .filter_map(|t| atom(module, *t))
                                    .map(str::to_string),
                            );
                            unused.extend(function_options(
                                module,
                                &args,
                                Some("nowarn_unused_function"),
                                &known,
                            )?);
                        } else {
                            let opts = args
                                .iter()
                                .filter_map(|t| atom(module, *t))
                                .map(str::to_string)
                                .collect::<Vec<_>>();
                            profile = registry.apply(&profile, &opts)?;
                            for (function, option) in function_options(module, &args, None, &known)?
                            {
                                if option != "nowarn_function" {
                                    registry.apply(
                                        &Profile::from_tags([]),
                                        std::slice::from_ref(&option),
                                    )?;
                                }
                                function_rules.entry(function).or_default().insert(option);
                            }
                        }
                    }
                }
                let mut warn_unused = true;
                for option in compile_options {
                    match option.as_str() {
                        "warn_unused_function" => warn_unused = true,
                        "nowarn_unused_function" => warn_unused = false,
                        _ => {}
                    }
                }
                let unused_functions = if warn_unused {
                    unused.into_iter().map(|(f, _)| f).collect()
                } else {
                    known.clone()
                };
                for function in unused_functions {
                    function_rules
                        .entry(function)
                        .or_default()
                        .insert("no_unused".into());
                }
                let mut functions = Vec::new();
                for (function, options) in function_rules {
                    let suppressed = options.contains("nowarn_function");
                    let function_profile = if suppressed {
                        profile.clone()
                    } else {
                        registry.apply(&profile, &options.into_iter().collect::<Vec<_>>())?
                    };
                    if suppressed || function_profile != profile {
                        functions.push(FunctionPolicy {
                            function,
                            profile: function_profile,
                            suppressed,
                        });
                    }
                }
                Ok(ModulePolicy {
                    module: module.name.clone(),
                    profile,
                    functions,
                })
            })();
            modules.push(result.map_err(|message| PolicyError {
                module: module.name.clone(),
                attribute,
                message,
            })?);
        }
        Ok(Self { modules })
    }
    pub fn allows(&self, function: &FunctionName, tag: Tag) -> bool {
        self.modules
            .iter()
            .find(|m| m.module == function.module)
            .is_some_and(|m| {
                m.functions
                    .iter()
                    .find(|f| &f.function == function)
                    .map_or_else(
                        || m.profile.contains(tag),
                        |f| !f.suppressed && f.profile.contains(tag),
                    )
            })
    }
}
