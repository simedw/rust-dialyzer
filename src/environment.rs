use crate::lattice::VariableId;
use crate::{
    core::{CoreModule, ErlangTerm, ErlangTerms, NodeId, TermId},
    forms::{Declaration, DeclarationKind, Forms, Record, RecordField},
    lattice::Type,
    program::{ModuleId, TypeName},
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, String>;
#[derive(Debug, Serialize)]
pub struct RecordPrototype {
    pub module: String,
    pub name: String,
    pub fields: Vec<Type>,
}
#[derive(Debug, Serialize)]
pub struct ImportReport {
    pub source_modules: usize,
    pub imported_modules: usize,
    pub excluded_modules: usize,
    pub declarations: usize,
    pub records: usize,
    pub nodes: usize,
    pub record_prototypes: Vec<RecordPrototype>,
    pub multi_arity_records: Vec<(String, String, Vec<usize>)>,
}
fn atom(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId) -> Result<&str> {
    match &terms[id.0] {
        ErlangTerm::Atom(s) => Ok(s),
        _ => Err("native environment expected atom".into()),
    }
}
fn tuple(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId, len: usize) -> Result<&[TermId]> {
    match &terms[id.0] {
        ErlangTerm::Tuple(xs) if xs.len() == len => Ok(xs),
        _ => Err(format!("native environment expected tuple of length {len}")),
    }
}
fn list(terms: &crate::shared_vec::SharedVec<ErlangTerm>, mut id: TermId) -> Result<Vec<TermId>> {
    let mut out = Vec::new();
    loop {
        match &terms[id.0] {
            ErlangTerm::Nil => return Ok(out),
            ErlangTerm::List { items, tail } => {
                out.extend(items);
                id = *tail
            }
            _ => return Err("native environment expected proper list".into()),
        }
    }
}
fn integer(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId) -> Result<usize> {
    match &terms[id.0] {
        ErlangTerm::Integer(value) => value
            .parse()
            .map_err(|_| "native environment expected nonnegative integer".into()),
        _ => Err("native environment expected integer".into()),
    }
}
pub fn exported_type_names(exports: &ErlangTerms) -> Result<BTreeSet<TypeName>> {
    let mut exported = BTreeSet::new();
    for id in list(&exports.terms, exports.root)? {
        let fields = tuple(&exports.terms, id, 3)?;
        exported.insert(TypeName {
            module: atom(&exports.terms, fields[0])?.into(),
            name: atom(&exports.terms, fields[1])?.into(),
            arity: integer(&exports.terms, fields[2])?,
        });
    }
    Ok(exported)
}
pub fn import(
    forms: &mut Forms,
    names: &mut Vec<String>,
    tables: &ErlangTerms,
    exports: &ErlangTerms,
    targets: &BTreeSet<String>,
) -> Result<ImportReport> {
    import_mode(forms, names, tables, exports, targets, false)
}
pub(crate) fn import_deferred(
    forms: &mut Forms,
    names: &mut Vec<String>,
    tables: &ErlangTerms,
    exports: &ErlangTerms,
    targets: &BTreeSet<String>,
) -> Result<ImportReport> {
    import_mode(forms, names, tables, exports, targets, true)
}
fn import_mode(
    forms: &mut Forms,
    names: &mut Vec<String>,
    tables: &ErlangTerms,
    exports: &ErlangTerms,
    targets: &BTreeSet<String>,
    deferred: bool,
) -> Result<ImportReport> {
    let ErlangTerm::Map(modules) = &tables.terms[tables.root.0] else {
        return Err("native type tables expected module map".into());
    };
    let exported = exported_type_names(exports)?
        .into_iter()
        .filter(|n| !targets.contains(&n.module))
        .collect::<BTreeSet<_>>();
    let mut candidate = forms.clone();
    let mut module_names = names.clone();
    let start = candidate.nodes.len();
    let mut report = ImportReport {
        source_modules: modules.len(),
        imported_modules: 0,
        excluded_modules: 0,
        declarations: 0,
        records: 0,
        nodes: 0,
        multi_arity_records: Vec::new(),
        record_prototypes: Vec::new(),
    };
    let mut source = CoreModule {
        name: String::new(),
        source_forms: tables.root,
        core: NodeId(0),
        terms: if deferred {
            crate::shared_vec::SharedVec::new()
        } else {
            tables.terms.clone()
        },
        map_iterator_orders: BTreeMap::new(),
        contract_variable_maps: BTreeMap::new(),
        nodes: Vec::new(),
    };
    for (module, table) in modules {
        let name = atom(&tables.terms, *module)?.to_owned();
        if targets.contains(&name) {
            report.excluded_modules += 1;
            continue;
        }
        report.imported_modules += 1;
        let mid = if let Some(i) = module_names.iter().position(|m| m == &name) {
            ModuleId(i)
        } else {
            let id = ModuleId(module_names.len());
            module_names.push(name.clone());
            id
        };
        source.name = name.clone();
        let ErlangTerm::Map(entries) = &tables.terms[table.0] else {
            return Err(format!("native module {name} expected type table map"));
        };
        // Native library tables are immutable metadata, decoded only on lookup.
        // Do not validate unused keys, headers, fields or cached values here.
        if deferred {
            continue;
        }
        for (key, value) in entries {
            let ErlangTerm::Tuple(key) = &tables.terms[key.0] else {
                return Err("native type table key expected tuple".into());
            };
            let tag = key
                .first()
                .map(|id| atom(&tables.terms, *id))
                .transpose()?
                .ok_or("empty type table key")?;
            match tag {
                "type" | "opaque" | "nominal" => {
                    if key.len() != 3 {
                        return Err("native declaration key must have three fields".into());
                    }
                    let typename = TypeName {
                        module: name.clone(),
                        name: atom(&tables.terms, key[1])?.into(),
                        arity: integer(&tables.terms, key[2])?,
                    };
                    let value = tuple(&tables.terms, *value, 2)?;
                    let metadata = tuple(&tables.terms, value[0], 4)?;
                    if atom(&tables.terms, metadata[0])? != name {
                        return Err("declaration module differs from type table owner".into());
                    }
                    let parameters = list(&tables.terms, metadata[3])?
                        .iter()
                        .map(|id| match &tables.terms[id.0] {
                            ErlangTerm::Atom(v) => Ok(VariableId::Atom(v.clone())),
                            ErlangTerm::Integer(v) => Ok(VariableId::Integer(
                                v.parse().map_err(|_| "invalid variable integer")?,
                            )),
                            _ => Err("type parameter must be atom or integer".into()),
                        })
                        .collect::<Result<Vec<_>>>()?;
                    if parameters.len() != typename.arity {
                        return Err("native declaration parameter arity mismatch".into());
                    }
                    let body = candidate.parse(&source, mid, metadata[2])?;
                    let exported = exported.contains(&typename);
                    let declaration = Declaration {
                        name: typename.clone(),
                        kind: match tag {
                            "type" => DeclarationKind::Alias,
                            "opaque" => DeclarationKind::Opaque,
                            _ => DeclarationKind::Nominal,
                        },
                        parameters,
                        body,
                        exported,
                        metadata: usize::MAX,
                    };
                    if candidate
                        .declarations
                        .insert(typename, declaration)
                        .is_some()
                    {
                        return Err(format!("duplicate imported declaration in {name}"));
                    }
                    report.declarations += 1;
                }
                "record" => {
                    if key.len() != 2 {
                        return Err("native record key must have two fields".into());
                    }
                    let record_name = atom(&tables.terms, key[1])?.to_owned();
                    let value = tuple(&tables.terms, *value, 2)?;
                    let variants = list(&tables.terms, value[1])?;
                    if variants.len() != 1 {
                        let arities = variants
                            .iter()
                            .map(|id| {
                                tuple(&tables.terms, *id, 2)
                                    .and_then(|fields| integer(&tables.terms, fields[0]))
                            })
                            .collect::<Result<Vec<_>>>()?;
                        report
                            .multi_arity_records
                            .push((name.clone(), record_name, arities));
                        continue;
                    }
                    let variant = tuple(&tables.terms, variants[0], 2)?;
                    let arity = integer(&tables.terms, variant[0])?;
                    let mut fields = Vec::new();
                    let mut types = Vec::new();
                    for id in list(&tables.terms, variant[1])? {
                        let field = tuple(&tables.terms, id, 3)?;
                        types.push(crate::type_exports::type_from_arena(tables, field[2])?);
                        fields.push(RecordField {
                            name: atom(&tables.terms, field[0])?.into(),
                            declared_type: Some(candidate.parse(&source, mid, field[1])?),
                            default_expression: None,
                        });
                    }
                    if fields.len() != arity {
                        return Err("native record field arity mismatch".into());
                    }
                    report.record_prototypes.push(RecordPrototype {
                        module: name.clone(),
                        name: record_name.clone(),
                        fields: types,
                    });
                    if candidate
                        .records
                        .insert(
                            (mid, record_name.clone()),
                            Record {
                                module: mid,
                                name: record_name,
                                fields,
                                metadata: usize::MAX,
                            },
                        )
                        .is_some()
                    {
                        return Err(format!("duplicate imported record in {name}"));
                    }
                    report.records += 1;
                }
                _ => return Err(format!("unknown native type table entry {tag}")),
            }
        }
    }
    report.nodes = candidate.nodes.len() - start;
    *forms = candidate;
    *names = module_names;
    Ok(report)
}
