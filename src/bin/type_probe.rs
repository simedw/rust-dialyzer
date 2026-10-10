use rust_dialyzer_poc::lattice::Type;
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn run(v: &Value) -> Result<Value, String> {
    let a = Type::from_value(&v["a"])?;
    let op = v["op"].as_str().ok_or("missing op")?;
    if op == "property" {
        let name = v["property"].as_str().ok_or("missing property")?;
        let result = match name {
            "idempotence" => a.join(&a)? == a && a.meet(&a)? == a,
            "identity" => a.join(&Type::none())? == a && a.meet(&Type::any())? == a,
            "limit" => a.limit(1000)? == a,
            other => {
                let b = Type::from_value(&v["b"])?;
                match other {
                    "absorption" => a.join(&a.meet(&b)?)? == a && a.meet(&a.join(&b)?)? == a,
                    "commutativity" => a.join(&b)? == b.join(&a)? && a.meet(&b)? == b.meet(&a)?,
                    "associativity" => {
                        let c = Type::from_value(&v["c"])?;
                        a.join(&b.join(&c)?)? == a.join(&b)?.join(&c)?
                            && a.meet(&b.meet(&c)?)? == a.meet(&b)?.meet(&c)?
                    }
                    _ => return Err("unsupported property".into()),
                }
            }
        };
        return Ok(json!(result));
    }
    if op == "unification_limit" {
        let binding = &v["binding"];
        let value = if binding["kind"] == "unification_nominal_set" {
            rust_dialyzer_poc::lattice::UnificationValue::MalformedNominalSet {
                nominal: Type::from_value(&binding["nominal"])?,
                structural: Type::from_value(&binding["structural"])?,
            }
        } else {
            rust_dialyzer_poc::lattice::UnificationValue::Type(Type::from_value(binding)?)
        };
        return Ok(value
            .limit(v["depth"].as_i64().ok_or("missing depth")?)?
            .to_value());
    }
    if op == "limit" {
        return Ok(a
            .limit(v["depth"].as_i64().ok_or("missing depth")?)?
            .to_value());
    }
    if op == "from_runtime_term" {
        let term = rust_dialyzer_poc::lattice::RuntimeTerm::from_value(&v["term"])?;
        return Ok(Type::from_runtime_term(&term)?.to_value());
    }
    if op == "widen_to_number" {
        return Ok(a.widen_to_number()?.to_value());
    }
    if op == "native_order" {
        return Ok(json!(
            match a.native_record_order(&Type::from_value(&v["b"])?) {
                std::cmp::Ordering::Less => "less",
                std::cmp::Ordering::Equal => "equal",
                std::cmp::Ordering::Greater => "greater",
            }
        ));
    }
    if op == "substitute_all_variables" {
        return Ok(a.substitute_all_variables()?.to_value());
    }
    if op == "meet_lists" {
        let read = |name: &str| -> Result<Vec<Type>, String> {
            v[name]
                .as_array()
                .ok_or("missing list")?
                .iter()
                .map(Type::from_value)
                .collect()
        };
        return Ok(json!(
            Type::meet_lists(&read("left")?, &read("right")?)?
                .iter()
                .map(Type::to_value)
                .collect::<Vec<_>>()
        ));
    }
    if op == "map_pairs" {
        let pairs = v["pairs"]
            .as_array()
            .ok_or("missing pairs")?
            .iter()
            .map(|pair| Ok((Type::from_value(&pair[0])?, Type::from_value(&pair[1])?)))
            .collect::<Result<Vec<_>, String>>()?;
        return Ok(Type::map_pairs(&pairs)?.to_value());
    }
    if op == "map_pairwise_merge" {
        let b = Type::from_value(&v["b"])?;
        let mode = v["mode"].as_str().ok_or("missing callback mode")?;
        let entries = a.map_pairwise_merge(&b, |_, p, x, q, y| {
            Ok(match mode {
                "skip" => None,
                "left" => Some((p, x.clone())),
                "right" => Some((q, y.clone())),
                "join" => Some((
                    if p == rust_dialyzer_poc::lattice::Presence::Mandatory
                        || q == rust_dialyzer_poc::lattice::Presence::Mandatory
                    {
                        rust_dialyzer_poc::lattice::Presence::Mandatory
                    } else {
                        rust_dialyzer_poc::lattice::Presence::Optional
                    },
                    x.join(y)?,
                )),
                _ => return Err(rust_dialyzer_poc::lattice::LatticeError::BadArg),
            })
        })?;
        return Ok(rust_dialyzer_poc::lattice::TypeValue::Entries(entries).to_value());
    }
    if op == "endpoint_min" || op == "endpoint_max" {
        let read = |key: &str| -> Result<rust_dialyzer_poc::lattice::Endpoint, String> {
            Ok(match v[key].as_str().ok_or("missing endpoint")? {
                "neg_inf" => rust_dialyzer_poc::lattice::Endpoint::NegativeInfinity,
                "pos_inf" => rust_dialyzer_poc::lattice::Endpoint::PositiveInfinity,
                n => rust_dialyzer_poc::lattice::Endpoint::Finite(
                    n.parse::<num_bigint::BigInt>().map_err(|e| e.to_string())?,
                ),
            })
        };
        let x = read("left")?;
        let y = read("right")?;
        return Ok(
            rust_dialyzer_poc::lattice::TypeValue::Endpoint(if op == "endpoint_min" {
                x.minimum(&y)
            } else {
                x.maximum(&y)
            })
            .to_value(),
        );
    }
    if op == "function_arity" {
        return Ok(Type::function_with_arity(
            v["arity"].as_i64().ok_or("missing arity")?,
            Type::from_value(&v["range"])?,
        )?
        .to_value());
    }
    if op == "atoms_constructor" {
        let values = v["values"]
            .as_array()
            .ok_or("missing atoms")?
            .iter()
            .map(|a| {
                a.as_str()
                    .map(str::to_owned)
                    .ok_or("invalid atom".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(Type::atoms(values).to_value());
    }
    if op == "number_literal" {
        let integer = v["value"]
            .as_str()
            .ok_or("missing integer")?
            .parse::<num_bigint::BigInt>()
            .map_err(|e| e.to_string())?;
        return Ok(Type::integer(integer).to_value());
    }
    if op == "type_to_string" || op == "record_field_diffs" || op == "atom_to_string" {
        let mut ctx = rust_dialyzer_poc::lattice::PresentationContext::default();
        if let Some(records) = v["records"].as_array() {
            for record in records {
                let fields = record["fields"]
                    .as_array()
                    .ok_or("missing record fields")?
                    .iter()
                    .map(|f| {
                        Ok(rust_dialyzer_poc::lattice::PresentationRecordField {
                            name: f["name"].as_str().ok_or("missing field name")?.into(),
                            declared_type: Type::from_value(&f["type"])?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                ctx.insert_record(
                    record["name"].as_str().ok_or("missing record name")?.into(),
                    fields,
                );
            }
        }
        return Ok(match op {
            "type_to_string" => json!(ctx.type_to_string(&a)?),
            "atom_to_string" => {
                json!(ctx.atom_to_string(v["name"].as_str().ok_or("missing atom")?))
            }
            _ => {
                let (positions, text) = a.record_field_diffs(&ctx)?;
                json!({"positions":positions,"text":text})
            }
        });
    }
    if op == "form_to_string" {
        let arena = rust_dialyzer_poc::core::ErlangTerms::parse(&v["arena"])?;
        return Ok(json!(rust_dialyzer_poc::presentation_form::form_to_string(
            &arena.terms,
            arena.root
        )?));
    }
    if op == "getter" {
        let kind = serde_json::from_value(v["name"].clone()).map_err(|e| e.to_string())?;
        return Ok(a.get(kind)?.to_value());
    }
    if op == "lift_list" {
        return Ok(a.lift_list_to_pos_empty()?.to_value());
    }
    if op == "is_native_type" {
        return Ok(json!(a.is_native_type()));
    }
    if [
        "supremum",
        "infimum",
        "subtract_types",
        "bitstr_concat_types",
        "any_none",
        "any_none_or_unit",
    ]
    .contains(&op)
    {
        let types = v["types"]
            .as_array()
            .ok_or("missing types")?
            .iter()
            .map(Type::from_value)
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(match op {
            "supremum" => Type::supremum(&types)?.to_value(),
            "infimum" => Type::infimum(&types)?.to_value(),
            "subtract_types" => a.subtract_types(&types)?.to_value(),
            "bitstr_concat_types" => Type::bitstr_concat_types(&types)?.to_value(),
            "any_none" => json!(Type::any_none(&types)),
            _ => json!(Type::any_none_or_unit(&types)),
        });
    }
    if op == "builtin" {
        let kind = serde_json::from_value(v["name"].clone()).map_err(|e| e.to_string())?;
        return Ok(Type::builtin(kind)?.to_value());
    }
    if op == "predicate" {
        let kind = serde_json::from_value(v["name"].clone()).map_err(|e| e.to_string())?;
        return Ok(json!(a.predicate(kind)?));
    }
    if op == "atom_literal" {
        return Ok(json!(
            a.is_atom_literal(v["name"].as_str().ok_or("missing name")?)?
        ));
    }
    if op == "same_opaque" {
        return Ok(json!(a.same_opaque(&Type::from_value(&v["b"])?)?));
    }
    if op == "unify" {
        let b = Type::from_value(&v["b"])?;
        let bindings = a.unify_bindings(&b).map_err(|e| e.to_string())?;
        return Ok(json!(bindings.into_iter().map(|(id,value)|json!({"id":match id{rust_dialyzer_poc::lattice::VariableId::Atom(a)=>json!({"atom":a}),rust_dialyzer_poc::lattice::VariableId::Integer(n)=>json!({"integer":n.to_string()})},"type":value.to_value()})).collect::<Vec<_>>()));
    }
    if op == "substitute" {
        let entries = v["bindings"].as_array().ok_or("missing bindings")?;
        let mut bindings = std::collections::BTreeMap::new();
        for e in entries {
            let var = Type::from_value(&json!({"kind":"var","id":e["id"]}))?;
            let mut names = std::collections::BTreeSet::new();
            var.append_var_names(&mut names);
            let id = names.into_iter().next().unwrap();
            bindings.insert(id, Type::from_value(&e["type"])?);
        }
        return Ok(a.substitute(&bindings)?.to_value());
    }
    if op == "has_var" {
        return Ok(json!(a.has_var()));
    }
    if op == "collect_var_names" {
        let mut names = std::collections::BTreeSet::new();
        a.append_var_names(&mut names);
        return Ok(json!(
            names.into_iter()
                .map(|id| match id {
                    rust_dialyzer_poc::lattice::VariableId::Atom(a) => json!({"atom":a}),
                    rust_dialyzer_poc::lattice::VariableId::Integer(n) =>
                        json!({"integer":n.to_string()}),
                })
                .collect::<Vec<_>>()
        ));
    }
    if op == "elements" {
        return Ok(json!(
            a.elements()?.iter().map(Type::to_value).collect::<Vec<_>>()
        ));
    }
    if op == "structural" {
        return Ok(a.structural()?.to_value());
    }
    if op == "is_opaque" {
        return Ok(json!(
            a.is_opaque(v["module"].as_str().ok_or("missing module")?)
        ));
    }
    if op == "canonical" {
        return Ok(a.to_value());
    }
    let b = Type::from_value(&v["b"])?;
    Ok(match op {
        "opacity_conflict" => json!(match a
            .opacity_conflict(&b, v["module"].as_str().ok_or("missing module")?)?
        {
            rust_dialyzer_poc::lattice::OpacityConflict::None => "none",
            rust_dialyzer_poc::lattice::OpacityConflict::ExpectedOpaque => "expected_opaque",
            rust_dialyzer_poc::lattice::OpacityConflict::ExpectedTransparent =>
                "expected_transparent",
        }),
        "map_get" => a.map_get(&b)?.to_value(),
        "map_is_key" => a.map_is_key(&b)?.to_value(),
        "map_remove" => a.map_remove(&b)?.to_value(),
        "map_put" => a.map_put(b, Type::from_value(&v["value"])?)?.to_value(),
        "map_update" => a.map_update(b, Type::from_value(&v["value"])?)?.to_value(),
        "bitstr_concat" => a.bitstr_concat(&b)?.to_value(),
        "bitstr_match" => a.bitstr_match(&b)?.to_value(),
        "join" => a.join(&b)?.to_value(),
        "meet" => a.meet(&b)?.to_value(),
        "subtract" => a.subtract(&b).map_err(|e| e.to_string())?.to_value(),
        "subtype" => json!(a.is_subtype(&b)?),
        _ => return Err(format!("unsupported operation {op}")),
    })
}
fn main() {
    for line in io::stdin().lock().lines() {
        let result = line
            .map_err(|e| e.to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
            .and_then(|v: Value| {
                let output = run(&v)?;
                let type_output = if output["kind"] == "type" {
                    &output["value"]
                } else {
                    &output
                };
                if v["native_output"] == true
                    && output.is_object()
                    && let Ok(t) = Type::from_value(type_output)
                {
                    Ok(json!({"native":t.to_native_value().to_value()}))
                } else {
                    Ok(output)
                }
            });
        println!(
            "{}",
            match result {
                Ok(v) => json!({"ok":v}),
                Err(e) => json!({"error":e}),
            }
        );
    }
}
