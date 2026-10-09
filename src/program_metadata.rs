use super::*;

impl Program {
    pub(super) fn index_metadata(&mut self, mid: ModuleId, module: &CoreModule) -> Result<()> {
        let CoreExpr::Module { attributes, .. } = &module.nodes[module.core.0].expr else {
            unreachable!()
        };
        let mut current_file = None;
        let mut behaviour_literals = None;
        for (key, value) in attributes {
            let name = literal_atom(module, *key)
                .ok_or_else(|| format!("module {}: nonatom attribute", module.name))?;
            let CoreExpr::Literal(raw) = module.nodes[value.0].expr else {
                unreachable!()
            };
            let kind = match name {
                "type" => MetadataKind::Type,
                "opaque" => MetadataKind::Opaque,
                "nominal" => MetadataKind::Nominal,
                "spec" => MetadataKind::Spec,
                "callback" => MetadataKind::Callback,
                "record" => MetadataKind::Record,
                "behaviour" | "behavior" => MetadataKind::Behaviour,
                "export_type" => MetadataKind::ExportType,
                "optional_callbacks" => MetadataKind::OptionalCallbacks,
                "dialyzer" => MetadataKind::Suppression,
                other => MetadataKind::Other(other.into()),
            };
            if self.target_contracts == TargetContracts::Ignore
                && matches!(
                    kind,
                    MetadataKind::Spec | MetadataKind::Callback | MetadataKind::OptionalCallbacks
                )
            {
                continue;
            }
            let mut origin = source_origin(module, module.nodes[key.0].annotations)?;
            if name == "file" {
                for item in list(module, raw)? {
                    if let Some(items) = tuple(module, item)
                        && items.len() == 2
                    {
                        current_file = Some(characters(module, items[0])?);
                    }
                }
            }
            if origin.file.is_none() {
                origin.file = current_file.clone();
            }
            let mut types = Vec::new();
            let mut functions = Vec::new();
            let mut behaviour = None;
            let mut records = Vec::new();
            match kind {
                MetadataKind::Type | MetadataKind::Opaque | MetadataKind::Nominal => {
                    for item in list(module, raw)? {
                        let fields=tuple(module,item).ok_or_else(||format!("module {} attribute {name} term{}: expected type declaration tuple",module.name,item.0))?;
                        if fields.len() != 3 {
                            return Err(format!(
                                "module {} attribute {name} term{}: expected name/body/parameters",
                                module.name, item.0
                            ));
                        }
                        let name = atom(module, fields[0]).ok_or_else(|| {
                            format!(
                                "module {} term{}: invalid type name",
                                module.name, fields[0].0
                            )
                        })?;
                        types.push(TypeName {
                            module: module.name.clone(),
                            name: name.into(),
                            arity: list(module, fields[2])?.len(),
                        });
                    }
                }
                MetadataKind::Spec | MetadataKind::Callback => {}
                MetadataKind::Record => {
                    for item in list(module, raw)? {
                        let fields = tuple(module, item).ok_or_else(|| {
                            format!(
                                "module {} record term{}: expected name/fields tuple",
                                module.name, item.0
                            )
                        })?;
                        if fields.len() != 2 {
                            return Err(format!(
                                "module {} record term{}: expected name/fields tuple",
                                module.name, item.0
                            ));
                        }
                        records.push(
                            atom(module, fields[0])
                                .ok_or_else(|| {
                                    format!(
                                        "module {} record term{}: nonatom name",
                                        module.name, item.0
                                    )
                                })?
                                .into(),
                        );
                    }
                }
                MetadataKind::Suppression => {
                    for item in list(module, raw)? {
                        if let Some(fields) = tuple(module, item)
                            && fields.len() == 2
                        {
                            for f in flattened(module, fields[1])? {
                                functions.push(function_term(module, f)?);
                            }
                        }
                    }
                }
                MetadataKind::ExportType => {
                    for item in list(module, raw)? {
                        let f = function_term(module, item)?;
                        types.push(TypeName {
                            module: f.module,
                            name: f.name,
                            arity: f.arity,
                        });
                    }
                }
                MetadataKind::OptionalCallbacks => {}
                MetadataKind::Behaviour => {
                    if behaviour_literals.is_none() {
                        behaviour_literals = Some(crate::raw_literal::RawLiteralArena::new(
                            std::sync::Arc::new(crate::core::ErlangTerms {
                                terms: module.terms.clone(),
                                root: module.source_forms,
                            }),
                        )?);
                    }
                    let literals = behaviour_literals
                        .as_ref()
                        .expect("behaviour arena initialized");
                    behaviour = Some(crate::behaviour_metadata::BehaviourAttribute {
                        payload: literals.at(raw)?,
                        annotations: literals.at(module.nodes[value.0].annotations)?,
                    });
                }
                _ => {}
            }
            let meta = self.metadata.len();
            if matches!(
                kind,
                MetadataKind::Type
                    | MetadataKind::Opaque
                    | MetadataKind::Nominal
                    | MetadataKind::Record
            ) {
                for target in remote_types(module, raw)? {
                    self.type_dependencies.insert(TypeDependency {
                        module: mid,
                        metadata: meta,
                        target,
                    });
                }
            }
            self.metadata.push(Metadata {
                module: mid,
                kind,
                raw,
                origin,
                types,
                functions,
                behaviour,
                records,
            });
        }
        Ok(())
    }
}

fn atom(module: &CoreModule, id: TermId) -> Option<&str> {
    if let ErlangTerm::Atom(s) = &module.terms[id.0] {
        Some(s)
    } else {
        None
    }
}
fn tuple(module: &CoreModule, id: TermId) -> Option<&[TermId]> {
    if let ErlangTerm::Tuple(xs) = &module.terms[id.0] {
        Some(xs)
    } else {
        None
    }
}
fn integer(module: &CoreModule, id: TermId) -> Option<i64> {
    if let ErlangTerm::Integer(s) = &module.terms[id.0] {
        s.parse().ok()
    } else {
        None
    }
}
fn list(module: &CoreModule, mut id: TermId) -> Result<Vec<TermId>> {
    let mut result = Vec::new();
    loop {
        match &module.terms[id.0] {
            ErlangTerm::Nil => return Ok(result),
            ErlangTerm::List { items, tail } => {
                result.extend(items);
                id = *tail;
            }
            _ => {
                return Err(format!(
                    "module {} term{}: expected proper list",
                    module.name, id.0
                ));
            }
        }
    }
}
fn flattened(module: &CoreModule, id: TermId) -> Result<Vec<TermId>> {
    let mut result = Vec::new();
    let mut pending = vec![id];
    while let Some(id) = pending.pop() {
        match module.terms[id.0] {
            ErlangTerm::List { .. } | ErlangTerm::Nil => {
                pending.extend(list(module, id)?.into_iter().rev())
            }
            _ => result.push(id),
        }
    }
    Ok(result)
}
fn characters(module: &CoreModule, id: TermId) -> Result<String> {
    characters_from_terms(&module.terms, id, &module.name)
}
pub(crate) fn characters_from_terms(
    terms: &crate::shared_vec::SharedVec<ErlangTerm>,
    root: TermId,
    module: &str,
) -> Result<String> {
    if let ErlangTerm::Binary(bytes) = &terms[root.0] {
        return String::from_utf8(bytes.clone())
            .map_err(|e| format!("module {module} term{}: nonUTF8 file: {e}", root.0));
    }
    let mut ids = Vec::new();
    let mut id = root;
    loop {
        match &terms[id.0] {
            ErlangTerm::Nil => break,
            ErlangTerm::List { items, tail } => {
                ids.extend(items.iter().copied());
                id = *tail;
            }
            _ => {
                return Err(format!(
                    "module {module} term{}: expected proper list",
                    root.0
                ));
            }
        }
    }
    ids.into_iter()
        .map(|id| match &terms[id.0] {
            ErlangTerm::Integer(v) => v
                .parse::<i64>()
                .ok()
                .and_then(|n| u32::try_from(n).ok())
                .and_then(char::from_u32)
                .ok_or_else(|| format!("module {module} term{}: invalid filename character", id.0)),
            _ => Err(format!(
                "module {module} term{}: invalid filename character",
                id.0
            )),
        })
        .collect()
}
pub(super) fn literal_atom(module: &CoreModule, node: NodeId) -> Option<&str> {
    if let CoreExpr::Literal(id) = module.nodes[node.0].expr {
        atom(module, id)
    } else {
        None
    }
}
pub(super) fn variable(module: &CoreModule, node: NodeId) -> Result<VariableName> {
    let CoreExpr::Var(id) = module.nodes[node.0].expr else {
        return Err(format!(
            "module {} node{}: expected variable",
            module.name, node.0
        ));
    };
    match &module.terms[id.0] {
        ErlangTerm::Atom(s) => Ok(VariableName::Atom(s.clone())),
        ErlangTerm::Integer(s) => Ok(VariableName::Integer(s.clone())),
        ErlangTerm::Tuple(_) => {
            let f = function_term(module, id)?;
            Ok(VariableName::Function(f.name, f.arity))
        }
        _ => Err(format!(
            "module {} term{}: invalid variable name",
            module.name, id.0
        )),
    }
}
pub(super) fn function_variable(module: &CoreModule, node: NodeId) -> Result<(String, usize)> {
    match variable(module, node)? {
        VariableName::Function(name, arity) => Ok((name, arity)),
        other => Err(format!(
            "module {} node{}: invalid function variable {other:?}",
            module.name, node.0
        )),
    }
}
fn function_term(module: &CoreModule, id: TermId) -> Result<FunctionName> {
    let fields = tuple(module, id).ok_or_else(|| {
        format!(
            "module {} term{}: invalid function/type identifier",
            module.name, id.0
        )
    })?;
    let (m, n, a) = match fields {
        [name, arity] => (module.name.as_str(), *name, *arity),
        [m, name, arity] => (
            atom(module, *m).ok_or_else(|| {
                format!("module {} term{}: invalid remote module", module.name, m.0)
            })?,
            *name,
            *arity,
        ),
        _ => {
            return Err(format!(
                "module {} term{}: expected function/type arity pair",
                module.name, id.0
            ));
        }
    };
    let name = atom(module, n).ok_or_else(|| {
        format!(
            "module {} term{}: invalid function/type name",
            module.name, n.0
        )
    })?;
    let arity = integer(module, a)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| format!("module {} term{}: invalid arity", module.name, a.0))?;
    Ok(FunctionName {
        module: m.into(),
        name: name.into(),
        arity,
    })
}
pub(super) fn source_origin(module: &CoreModule, raw: TermId) -> Result<SourceOrigin> {
    let mut origin = SourceOrigin {
        raw,
        file: None,
        line: None,
        column: None,
        generated: false,
    };
    for id in list(module, raw)? {
        if atom(module, id) == Some("compiler_generated") {
            origin.generated = true;
        }
        if origin.line.is_none()
            && let Some(line) = integer(module, id)
        {
            origin.line = Some(line);
            continue;
        }
        if let Some(fields) = tuple(module, id)
            && fields.len() == 2
        {
            if origin.line.is_none()
                && let (Some(line), Some(col)) =
                    (integer(module, fields[0]), integer(module, fields[1]))
            {
                origin.line = Some(line);
                origin.column = u64::try_from(col).ok();
            }
            if atom(module, fields[0]) == Some("file") {
                origin.file = Some(characters(module, fields[1])?);
            }
            if atom(module, fields[0]) == Some("generated")
                && atom(module, fields[1]) == Some("true")
            {
                origin.generated = true;
            }
        }
    }
    origin.generated |= origin.line.unwrap_or(0) < 1;
    Ok(origin)
}
fn ast_annotation(module: &CoreModule, id: TermId) -> bool {
    integer(module, id).is_some()
        || tuple(module, id).is_some_and(|fields| {
            fields.len() == 2
                && integer(module, fields[0]).is_some()
                && integer(module, fields[1]).is_some()
        })
}
fn remote_types(module: &CoreModule, root: TermId) -> Result<BTreeSet<TypeName>> {
    let mut found = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut stack = vec![root];
    while let Some(id) = stack.pop() {
        if !seen.insert(id.0) {
            continue;
        }
        if let Some(fields) = tuple(module, id)
            && fields.len() == 3
            && atom(module, fields[0]) == Some("remote_type")
            && ast_annotation(module, fields[1])
        {
            let args = list(module, fields[2])?;
            if args.len() != 3 {
                return Err(format!(
                    "module {} remote_type term{}: expected module/name/arguments",
                    module.name, id.0
                ));
            }
            let parsed_atom = |id| -> Result<String> {
                let node = tuple(module, id).ok_or_else(|| {
                    format!(
                        "module {} remote_type term{}: expected atom AST",
                        module.name, id.0
                    )
                })?;
                if node.len() != 3 || atom(module, node[0]) != Some("atom") {
                    return Err(format!(
                        "module {} remote_type term{}: expected atom AST",
                        module.name, id.0
                    ));
                }
                Ok(atom(module, node[2])
                    .ok_or_else(|| {
                        format!(
                            "module {} remote_type term{}: nonatom name",
                            module.name, id.0
                        )
                    })?
                    .into())
            };
            found.insert(TypeName {
                module: parsed_atom(args[0])?,
                name: parsed_atom(args[1])?,
                arity: list(module, args[2])?.len(),
            });
        }
        if let Some(fields) = tuple(module, id)
            && fields.len() == 4
            && atom(module, fields[0]) == Some("user_type")
            && ast_annotation(module, fields[1])
        {
            found.insert(TypeName {
                module: module.name.clone(),
                name: atom(module, fields[2])
                    .ok_or_else(|| {
                        format!(
                            "module {} user_type term{}: invalid name",
                            module.name, id.0
                        )
                    })?
                    .into(),
                arity: list(module, fields[3])?.len(),
            });
        }
        match &module.terms[id.0] {
            ErlangTerm::Tuple(ids) => stack.extend(ids),
            ErlangTerm::List { items, tail } => {
                stack.extend(items);
                stack.push(*tail);
            }
            ErlangTerm::Map(entries) => stack.extend(entries.iter().flat_map(|(k, v)| [*k, *v])),
            _ => {}
        }
    }
    Ok(found)
}
