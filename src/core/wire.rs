use super::*;
use serde_json::json;

impl CoreExpr {
    fn wire_groups(&self) -> Vec<Vec<NodeId>> {
        match self {
            Self::Literal(_) | Self::Var(_) | Self::Opaque(_) => vec![],
            Self::Values(v) | Self::Binary(v) | Self::Tuple(v) => vec![v.clone()],
            Self::Bitstr {
                value,
                size,
                unit,
                typ,
                flags,
            } => vec![
                vec![*value],
                vec![*size],
                vec![*unit],
                vec![*typ],
                vec![*flags],
            ],
            Self::Cons { head, tail } => vec![vec![*head], vec![*tail]],
            Self::Map { pairs, .. } => vec![pairs.clone()],
            #[cfg(otp_domain = "29")]
            Self::Record { pairs, .. } => vec![pairs.clone()],
            #[cfg(otp_domain = "29")]
            Self::RecordPair { key, value } => vec![vec![*key], vec![*value]],
            Self::MapPair {
                operation,
                key,
                value,
            } => vec![vec![*operation], vec![*key], vec![*value]],
            Self::Let {
                vars,
                argument,
                body,
            } => vec![vars.clone(), vec![*argument], vec![*body]],
            Self::Seq { argument, body } => vec![vec![*argument], vec![*body]],
            Self::Apply { operator, args } => vec![vec![*operator], args.clone()],
            Self::Call { module, name, args } => vec![vec![*module], vec![*name], args.clone()],
            Self::Primop { name, args } => vec![vec![*name], args.clone()],
            Self::Case { argument, clauses } => vec![vec![*argument], clauses.clone()],
            Self::Clause {
                patterns,
                guard,
                body,
            } => vec![patterns.clone(), vec![*guard], vec![*body]],
            Self::Alias { variable, pattern } => vec![vec![*variable], vec![*pattern]],
            Self::Fun { params, body } => vec![params.clone(), vec![*body]],
            Self::Receive {
                clauses,
                timeout,
                action,
            } => vec![clauses.clone(), vec![*timeout], vec![*action]],
            Self::Try(try_payload) => { let crate::core::TryExpr {
                argument,
                vars,
                body,
                exception_vars,
                handler,
            } = try_payload.as_ref(); vec![
                vec![*argument],
                vars.clone(),
                vec![*body],
                exception_vars.clone(),
                vec![*handler],
            ] },
            Self::Catch { body } => vec![vec![*body]],
            Self::Letrec { definitions, body } => vec![
                definitions.iter().flat_map(|(k, v)| [*k, *v]).collect(),
                vec![*body],
            ],
            Self::Module(module_payload) => { let crate::core::ModuleExpr {
                name,
                exports,
                attributes,
                definitions,
            } = module_payload.as_ref(); vec![
                vec![*name],
                exports.clone(),
                attributes.iter().flat_map(|(k, v)| [*k, *v]).collect(),
                definitions.iter().flat_map(|(k, v)| [*k, *v]).collect(),
            ] },
        }
    }
}
impl CoreModule {
    pub fn to_wire_value(&self) -> Value {
        // Core emission preserves every table entry, including backing terms.
        let terms = self
            .terms
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut value = crate::type_exports::term_to_value(t, |id| id.0);
                if let ErlangTerm::Map(_) = t {
                    value["iterator_order"] = json!(self.map_iterator_orders.get(&TermId(i)));
                }
                value
            })
            .collect::<Vec<_>>();
        let nodes = self.nodes.iter().map(|node| {
            let mut value=json!({"kind":node.expr.kind(),"annotations":node.annotations.0,
                "groups":node.expr.wire_groups().iter().map(|g|g.iter().map(|id|id.0).collect::<Vec<_>>()).collect::<Vec<_>>()});
            match &node.expr {
                CoreExpr::Literal(t)|CoreExpr::Var(t)|CoreExpr::Opaque(t) => value["value"]=json!(t.0),
                CoreExpr::Map {base,pattern,..} => { value["map_arg"]=json!(base.0);value["map_pattern"]=json!(pattern); },
                #[cfg(otp_domain = "29")]
                CoreExpr::Record {argument,id,..} => { value["record_arg"]=json!(argument.0);value["record_id"]=json!(id.0); },
                _=>{}
            }
            value
        }).collect::<Vec<_>>();
        json!({"name":self.name,"source_forms":self.source_forms.0,"core":self.core.0,
            "terms":terms,"nodes":nodes,"contract_variable_maps":self.contract_variable_maps.iter().map(|(k,v)|[k.0,v.0]).collect::<Vec<_>>()})
    }
}
impl CoreProgram {
    pub fn to_wire_value(&self, context: &Value) -> Value {
        let mut wire = context.clone();
        wire["modules"] =
            Value::Array(self.modules.iter().map(CoreModule::to_wire_value).collect());
        wire
    }
}

/// A path formatted only when an error message needs it; validation runs
/// over every term and node, and eager path strings dominated its cost.
#[derive(Clone, Copy)]
struct Lazy<F>(F);
impl<F: Fn(&mut std::fmt::Formatter<'_>) -> std::fmt::Result> std::fmt::Display for Lazy<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (self.0)(f)
    }
}

fn checked(id: usize, before: usize, path: impl std::fmt::Display) -> Result<()> {
    if id >= before {
        Err(format!(
            "{path}: index {id} must refer to earlier entry below {before}"
        ))
    } else {
        Ok(())
    }
}
impl CoreModule {
    pub fn validate(&self, path: &str, _version: CoreVersion) -> Result<()> {
        for (i, t) in self.terms.iter().enumerate() {
            let at = Lazy(move |f: &mut std::fmt::Formatter<'_>| write!(f, "{path}.terms[{i}]"));
            match t {
                ErlangTerm::Tuple(v) => {
                    for (j, id) in v.iter().enumerate() {
                        checked(id.0, i, format_args!("{at}.items[{j}]"))?;
                    }
                }
                ErlangTerm::List { items, tail } => {
                    for (j, id) in items.iter().enumerate() {
                        checked(id.0, i, format_args!("{at}.items[{j}]"))?;
                    }
                    checked(tail.0, i, format_args!("{at}.tail"))?;
                }
                ErlangTerm::Map(v) => {
                    for (j, (k, v)) in v.iter().enumerate() {
                        checked(k.0, i, format_args!("{at}.entries[{j}][0]"))?;
                        checked(v.0, i, format_args!("{at}.entries[{j}][1]"))?;
                    }
                }
                #[cfg(otp_domain = "29")]
                ErlangTerm::Record(record) => {
                    if record.module.chars().count() > 255 || record.name.chars().count() > 255 { return Err(format!("{at}: record atom exceeds 255 characters")); }
                    let mut names = std::collections::BTreeSet::new();
                    for (name, value) in &record.fields {
                        if name.chars().count() > 255 || !names.insert(name) { return Err(format!("{at}: invalid or duplicate record field")); }
                        checked(value.0, i, format_args!("{at}.record_field"))?;
                    }
                }
                ErlangTerm::Integer(n) => {
                    let digits = n.strip_prefix('-').unwrap_or(n);
                    if digits.is_empty()
                        || !digits.bytes().all(|b| b.is_ascii_digit())
                        || (digits.len() > 1 && digits.starts_with('0'))
                        || n == "-0"
                    {
                        return Err(format!("{at}.value: expected canonical decimal integer"));
                    }
                }
                ErlangTerm::ExternalFun(external_fun_payload)
                    if external_fun_payload.module.chars().count() > 255
                        || external_fun_payload.function.chars().count() > 255 =>
                {
                    return Err(format!(
                        "{at}: external function atom exceeds 255 characters"
                    ));
                }
                ErlangTerm::Bitstring { bytes, bits } => {
                    if bits.div_ceil(8) != bytes.len() as u64 {
                        return Err(format!("{at}.bits: length does not match padded bytes"));
                    }
                    let pad = (8 - bits % 8) % 8;
                    if pad != 0 && bytes.last().is_some_and(|b| b & ((1 << pad) - 1) != 0) {
                        return Err(format!("{at}.bytes: nonzero bitstring padding"));
                    }
                }
                _ => {}
            }
        }
        for (i, t) in self.terms.iter().enumerate() {
            if let ErlangTerm::Map(pairs) = t {
                let at = Lazy(move |f: &mut std::fmt::Formatter<'_>| write!(f, "{path}.terms[{i}].iterator_order"));
                let order = self
                    .map_iterator_orders
                    .get(&TermId(i))
                    .ok_or_else(|| format!("{at}.iterator_order: missing field"))?;
                if order.len() != pairs.len() {
                    return Err(format!("{at}: expected a complete pair permutation"));
                }
                let mut seen = vec![false; pairs.len()];
                for (k, index) in order.iter().enumerate() {
                    checked(*index, pairs.len(), format_args!("{at}[{k}]"))?;
                    if std::mem::replace(&mut seen[*index], true) {
                        return Err(format!("{at}[{k}]: repeated pair index"));
                    }
                }
            }
        }
        checked(
            self.source_forms.0,
            self.terms.len(),
            format_args!("{path}.source_forms"),
        )?;
        proper_list(
            self.source_forms,
            &self.terms,
            format_args!("{path}.source_forms"),
        )?;
        for (i, (form, map)) in self.contract_variable_maps.iter().enumerate() {
            let at = Lazy(move |f: &mut std::fmt::Formatter<'_>| write!(f, "{path}.contract_variable_maps[{i}]"));
            checked(form.0, self.terms.len(), &at)?;
            checked(map.0, self.terms.len(), &at)?;
            let ErlangTerm::Tuple(fields) = &self.terms[form.0] else {
                return Err(format!("{at}: expected bounded_fun AST"));
            };
            if fields.len() != 4
                || !matches!(&self.terms[fields[0].0],ErlangTerm::Atom(a) if a=="type")
                || !matches!(&self.terms[fields[2].0],ErlangTerm::Atom(a) if a=="bounded_fun")
            {
                return Err(format!("{at}: expected bounded_fun AST"));
            }
            let ErlangTerm::Map(pairs) = &self.terms[map.0] else {
                return Err(format!("{at}: expected compiler variable map"));
            };
            if pairs.iter().any(|(k, v)| {
                !matches!(&self.terms[k.0],ErlangTerm::Atom(a) if a!="_")
                    && !matches!(&self.terms[k.0], ErlangTerm::Integer(_))
                    || !matches!(&self.terms[v.0],ErlangTerm::Atom(a) if a=="nil")
            }) {
                return Err(format!(
                    "{at}: expected variable keys and nil placeholder values"
                ));
            }
            if !self.map_iterator_orders.contains_key(map) {
                return Err(format!("{at}: variable map lacks iterator provenance"));
            }
        }
        for (i, node) in self.nodes.iter().enumerate() {
            let at = Lazy(move |f: &mut std::fmt::Formatter<'_>| write!(f, "{path}.nodes[{i}]"));
            if node.id.0 != i {
                return Err(format!("{at}: node ID differs from table position"));
            }
            checked(
                node.annotations.0,
                self.terms.len(),
                format_args!("{at}.annotations"),
            )?;
            proper_list(node.annotations, &self.terms, format_args!("{at}.annotations"))?;
            let groups = node.expr.wire_groups();
            for (g, ids) in groups.iter().enumerate() {
                for (j, id) in ids.iter().enumerate() {
                    checked(id.0, i, format_args!("{at}.groups[{g}][{j}]"))?;
                }
            }
            // Every id below was checked to precede node i.
            let prior = &self.nodes;
            match &node.expr {
                CoreExpr::Literal(t) | CoreExpr::Opaque(t) => {
                    checked(t.0, self.terms.len(), format_args!("{at}.value"))?
                }
                CoreExpr::Var(t) => {
                    checked(t.0, self.terms.len(), format_args!("{at}.value"))?;
                    if !valid_var_name(*t, &self.terms) {
                        return Err(format!(
                            "{at}.value: expected atom, integer or atom/arity tuple variable name"
                        ));
                    }
                }
                CoreExpr::Binary(ids) => {
                    require_kinds(prior, ids, "bitstr", format_args!("{at}.groups[0]"))?
                }
                CoreExpr::Map { base, pairs, .. } => {
                    require_kinds(prior, pairs, "map_pair", format_args!("{at}.groups[0]"))?;
                    checked(base.0, i, format_args!("{at}.map_arg"))?;
                }
                #[cfg(otp_domain = "29")]
                CoreExpr::Record { argument, id, pairs } => {
                    require_kinds(prior, pairs, "record_pair", format_args!("{at}.groups[0]"))?;
                    checked(argument.0, i, format_args!("{at}.record_arg"))?;
                    checked(id.0, i, format_args!("{at}.record_id"))?;
                }
                CoreExpr::Let { vars, .. } | CoreExpr::Fun { params: vars, .. } => {
                    require_kinds(prior, vars, "var", format_args!("{at}.groups[0]"))?
                }
                CoreExpr::Case { clauses, .. } => {
                    require_kinds(prior, clauses, "clause", format_args!("{at}.groups[1]"))?
                }
                CoreExpr::Receive { clauses, .. } => {
                    require_kinds(prior, clauses, "clause", format_args!("{at}.groups[0]"))?
                }
                CoreExpr::Alias { variable, .. } => {
                    require_kind(prior, *variable, "var", format_args!("{at}.groups[0][0]"))?
                }
                CoreExpr::Try(try_payload) => {
                    let crate::core::TryExpr {
                    vars,
                    exception_vars,
                    ..
                } = try_payload.as_ref();

                    require_kinds(prior, vars, "var", format_args!("{at}.groups[1]"))?;
                    require_kinds(prior, exception_vars, "var", format_args!("{at}.groups[3]"))?;
                }
                CoreExpr::Letrec { .. } => {
                    definition_pairs(
                        prior,
                        &self.terms,
                        groups[0].clone(),
                        format_args!("{at}.groups[0]"),
                    )?;
                }
                CoreExpr::Module(module_payload) => {
                    let crate::core::ModuleExpr {
                    name,
                    exports,
                    attributes,
                    ..
                } = module_payload.as_ref();

                    if literal_atom(*name, prior, &self.terms).is_none() {
                        return Err(format!(
                            "{at}.groups[0][0]: expected literal atom module name"
                        ));
                    }
                    for (j, id) in exports.iter().enumerate() {
                        if function_name(*id, prior, &self.terms).is_none() {
                            return Err(format!(
                                "{at}.groups[1][{j}]: expected atom/arity function variable"
                            ));
                        }
                    }
                    for (j, (k, v)) in attributes.iter().enumerate() {
                        if literal_atom(*k, prior, &self.terms).is_none() {
                            return Err(format!(
                                "{at}.groups[2][{}]: expected literal atom attribute name",
                                j * 2
                            ));
                        }
                        require_kind(
                            prior,
                            *v,
                            "literal",
                            format_args!("{at}.groups[2][{}]", j * 2 + 1),
                        )?;
                    }
                    let definitions = definition_pairs(
                        prior,
                        &self.terms,
                        groups[3].clone(),
                        format_args!("{at}.groups[3]"),
                    )?;
                    for (j, id) in exports.iter().enumerate() {
                        let function = function_name(*id, prior, &self.terms).unwrap();
                        if !definitions.iter().any(|(key, _)| {
                            function_name(*key, prior, &self.terms) == Some(function)
                        }) {
                            return Err(format!(
                                "{at}.groups[1][{j}]: exported function has no definition"
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
        checked(self.core.0, self.nodes.len(), format_args!("{path}.core"))?;
        match &self.nodes[self.core.0].expr {
            CoreExpr::Module(module_payload)
                if literal_atom(module_payload.name, &self.nodes, &self.terms) == Some(self.name.as_str()) =>
            {
                Ok(())
            }
            CoreExpr::Module(_) => Err(format!(
                "{path}.core: Core module name differs from {}",
                self.name
            )),
            _ => Err(format!("{path}.core: expected module node")),
        }
    }
}
