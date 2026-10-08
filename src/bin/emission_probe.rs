use rust_dialyzer_poc::{
    bifs::boundary::NativeTerm,
    core::ErlangTerms,
    emission::{self, FunctionEvidence, ParentEvidence, SourceAnnotation, WarningState},
    returns::ReturnKind,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn term(value: &Value) -> NativeTerm {
    let arena = ErlangTerms::parse(value).unwrap();
    NativeTerm::from_arena(&arena, arena.root).unwrap()
}
fn atom(value: &NativeTerm) -> &str {
    let NativeTerm::Atom { value: a } = value else {
        panic!("not atom")
    };
    a
}
fn warnings(value: NativeTerm) -> Vec<NativeTerm> {
    match value {
        NativeTerm::Nil => vec![],
        NativeTerm::List { items, tail } => {
            assert!(matches!(*tail, NativeTerm::Nil));
            items
        }
        _ => panic!("warnings not list"),
    }
}
fn run(row: Value) -> Value {
    let data = &row["request"];
    if data["operation"] == "policy" {
        let flag = |name: &str| data[name].as_bool().unwrap();
        let prop = |name: &str| match data[name].as_str().unwrap() {
            "enabled" => emission::PropertyOverride::Enabled,
            "disabled" => emission::PropertyOverride::Disabled,
            _ => emission::PropertyOverride::Inherit,
        };
        let facts = emission::PolicyFacts {
            named_owner: flag("named"),
            module_metadata_present: flag("module_metadata"),
            module_tag_enabled: flag("module_tag"),
            module_function_suppressed: flag("module_suppressed"),
            function_tag: prop("function_tag"),
            function_suppression: prop("function_suppression"),
        };
        let native = !warnings(term(&row["warnings"])).is_empty();
        return json!({"id":row["id"],"matched":native==facts.allows()});
    }
    if data["operation"] == "collection" {
        let strings = |name: &str| {
            data[name]
                .as_array()
                .unwrap()
                .iter()
                .map(|value| value.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        };
        let old = strings("old");
        let mut state = WarningState {
            warning_mode: false,
            warnings: old.clone(),
        };
        state.add_all(strings("new"));
        let extended = state.warnings.clone();
        let added = state.remove_added(&old);
        let NativeTerm::Tuple { items } = term(&row["warnings"]) else {
            panic!()
        };
        let atoms = |value: NativeTerm| {
            warnings(value)
                .iter()
                .map(|value| atom(value).to_string())
                .collect::<Vec<_>>()
        };
        let matched = extended == atoms(items[1].clone())
            && added == atoms(items[2].clone())
            && state.warnings == atoms(items[3].clone());
        return json!({"id":row["id"],"matched":matched});
    }
    let NativeTerm::Tuple { items: types } = term(&row["input_types"]) else {
        panic!()
    };
    let result = types[0].type_value().unwrap();
    let own = (!matches!(&types[1],NativeTerm::Atom {value} if value == "nil"))
        .then(|| types[1].type_value().unwrap());
    let parent = (!matches!(&types[2],NativeTerm::Atom {value} if value == "nil"))
        .then(|| types[2].type_value().unwrap());
    let kind = match data["return_kind"].as_str().unwrap_or("only_normal") {
        "no_match" => ReturnKind::NoMatch,
        "only_explicit" => ReturnKind::OnlyExplicit,
        "both" => ReturnKind::Both,
        _ => ReturnKind::OnlyNormal,
    };
    let flag = |name: &str| data[name].as_bool().unwrap_or(false);
    let evidence = FunctionEvidence {
        named: flag("named"),
        handled: flag("handled"),
        reachable: flag("reachable"),
        result: &result,
        validated_own_contract_return: own.as_ref(),
        parent: ParentEvidence {
            escaping: flag("escaping"),
            incoming_count: data["incoming_count"].as_u64().unwrap_or(0) as usize,
            parent_named: flag("parent_named"),
            validated_parent_contract_return: parent.as_ref(),
        },
        return_kind: kind,
    };
    let annotations = data["annotations"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![json!(1)]);
    let annotation = SourceAnnotation {
        line: annotations.iter().find_map(|a| {
            a.as_i64()
                .or_else(|| a.as_array().and_then(|a| a.first()).and_then(Value::as_i64))
        }),
        compiler_generated: annotations.iter().any(|a| a == "compiler_generated"),
    };
    let mut state = WarningState {
        warning_mode: flag("warning_mode"),
        warnings: vec![],
    };
    if data["operation"] == "add" {
        state.add(annotation, "warn_failing_call".to_string(), flag("forced"));
    } else if data["operation"] == "compare" {
        let op = match data["operator"].as_str().unwrap() {
            "=:=" => emission::EqualityOperator::ExactEqual,
            "=/=" => emission::EqualityOperator::ExactNotEqual,
            "==" => emission::EqualityOperator::Equal,
            "/=" => emission::EqualityOperator::NotEqual,
            _ => panic!(),
        };
        let right = types[3].type_value().unwrap();
        if data["module"].as_str().unwrap_or("erlang") == "erlang" {
            for warning in emission::comparison_warnings(&result, op, &right, "m")
                .unwrap()
                .into_iter()
                .rev()
            {
                state.add(
                    annotation,
                    match warning {
                        emission::ComparisonWarning::Opaque { .. } => "warn_opaque",
                        emission::ComparisonWarning::Exact { .. } => "warn_matching",
                    }
                    .into(),
                    false,
                );
            }
        }
    } else if let Some(warning) = emission::function_warning(&evidence).unwrap() {
        state.add(
            annotation,
            match warning {
                emission::FunctionWarning::Unused => "warn_not_called",
                emission::FunctionWarning::OnlyExit => "warn_return_only_exit",
                emission::FunctionWarning::NoReturn { .. } => "warn_return_no_exit",
            }
            .to_string(),
            false,
        );
    }
    let native = warnings(term(&row["warnings"]));
    let mut payloads_match = true;
    if data["operation"] == "compare" && !native.is_empty() {
        let op = match data["operator"].as_str().unwrap() {
            "=:=" => emission::EqualityOperator::ExactEqual,
            "=/=" => emission::EqualityOperator::ExactNotEqual,
            "==" => emission::EqualityOperator::Equal,
            "/=" => emission::EqualityOperator::NotEqual,
            _ => panic!(),
        };
        let right = types[3].type_value().unwrap();
        let expected = emission::comparison_warnings(&result, op, &right, "m").unwrap();
        payloads_match = expected.len() == native.len();
        for (warning, expected) in native.iter().zip(expected) {
            let NativeTerm::Tuple { items: w } = warning else {
                panic!()
            };
            let NativeTerm::Tuple { items: message } = &w[2] else {
                panic!()
            };
            let NativeTerm::List {
                items: arguments, ..
            } = &message[1]
            else {
                panic!()
            };
            let (kind, left, right) = match expected {
                emission::ComparisonWarning::Opaque { left, right, .. } => {
                    ("opaque_compare", left, right)
                }
                emission::ComparisonWarning::Exact { left, right, .. } => {
                    ("exact_compare", left, right)
                }
            };
            payloads_match &= atom(&message[0]) == kind
                && arguments[0].type_value().unwrap() == left
                && atom(&arguments[1]) == data["operator"].as_str().unwrap()
                && arguments[2].type_value().unwrap() == right;
        }
    } else if data["operation"] != "add" && !native.is_empty() {
        let NativeTerm::Tuple { items: w } = &native[0] else {
            panic!()
        };
        let NativeTerm::Tuple { items: message } = &w[2] else {
            panic!()
        };
        let NativeTerm::List {
            items: arguments, ..
        } = &message[1]
        else {
            panic!()
        };
        if flag("handled") {
            payloads_match = atom(&message[0]) == "no_return"
                && atom(&arguments[0]) == data["return_kind"].as_str().unwrap_or("only_normal")
                && arguments.len() == if flag("named") { 3 } else { 1 };
        } else {
            payloads_match = atom(&message[0]) == "unused_fun" && arguments.len() == 2;
        }
    }
    let tags: Vec<_> = native
        .iter()
        .map(|w| {
            let NativeTerm::Tuple { items: w } = w else {
                panic!()
            };
            atom(&w[0]).to_string()
        })
        .collect();
    json!({"id":row["id"],"matched":tags==state.warnings && payloads_match,"native":tags,"rust":state.warnings})
}
fn main() {
    for line in io::stdin().lock().lines() {
        let row: Value = serde_json::from_str(&line.unwrap()).unwrap();
        println!("{}", run(row));
    }
}
