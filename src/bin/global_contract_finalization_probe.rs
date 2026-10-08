use rust_dialyzer_poc::{
    compiled_definitions::{ModuleCompletionOrder, PreparedDeclaration, PreparedDefinitions},
    core::{CoreProgram, ErlangTerm, ErlangTerms},
    dependencies::{ImportPlan, PriorPlt},
    diagnostic_inputs::TargetCallbackState,
    native_form_environment::RawFormTerm,
    native_runtime_hash::NativeAtomLayout,
    program::{ModuleId, Program},
    term_order::compare_map_keys,
};
use serde_json::{Value, json};
use std::io::{self, BufRead};
fn matches_map<'a>(
    entries: impl Iterator<Item = &'a PreparedDeclaration>,
    expected: &RawFormTerm,
) -> Result<bool, String> {
    let ErlangTerm::Map(pairs) = expected.term() else {
        return Err("expected contract map".into());
    };
    let entries = entries.collect::<Vec<_>>();
    if entries.len() != pairs.len() {
        return Ok(false);
    }
    for entry in entries {
        let pair = pairs.iter().find(|(key, _)| {
            compare_map_keys(
                entry.key.raw().arena(),
                entry.key.raw().id(),
                expected.arena(),
                *key,
            )
            .is_ok_and(|c| c.is_eq())
        });
        let Some((_, value)) = pair else {
            return Ok(false);
        };
        let actual = entry.callback_row();
        let ErlangTerm::Tuple(fields) = actual.term() else {
            unreachable!()
        };
        if !compare_map_keys(actual.arena(), fields[1], expected.arena(), *value)?.is_eq() {
            return Ok(false);
        }
    }
    Ok(true)
}
fn literal(value: &Value) -> Result<RawFormTerm, String> {
    let terms = std::sync::Arc::new(ErlangTerms::parse(value)?);
    RawFormTerm::new(terms.clone(), terms.root)
}
fn equals(actual: &RawFormTerm, expected: &Value) -> Result<bool, String> {
    let expected = ErlangTerms::parse(expected)?;
    Ok(compare_map_keys(actual.arena(), actual.id(), &expected, expected.root)?.is_eq())
}
fn main() -> Result<(), String> {
    let data = std::fs::read_to_string(std::env::args().nth(1).ok_or("library")?)
        .map_err(|e| e.to_string())?;
    for line in io::stdin().lock().lines() {
        let row: Value =
            serde_json::from_str(&line.map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        let core = CoreProgram::parse(&row["core"].to_string())?;
        let program = Program::build(&core)?;
        let library =
            PriorPlt::parse(&data, &core, &ImportPlan::cold_targets(&core))?.into_cold_store()?;
        let completion = ModuleCompletionOrder::checked(
            &core,
            row["completion"]
                .as_array()
                .ok_or("completion")?
                .iter()
                .map(|name| {
                    core.modules
                        .iter()
                        .position(|m| m.name == name.as_str().unwrap())
                        .map(ModuleId)
                        .ok_or("unknown completion module")
                })
                .collect::<Result<Vec<_>, _>>()?,
        )?;
        let definitions =
            PreparedDefinitions::prepare_with_completion(&core, &program, &library, &completion)
                .map_err(|e| e.to_string())?;
        let layout =
            NativeAtomLayout::parse(&row["core"]["atom_layout"]).map_err(|e| e.to_string())?;
        let callbacks = TargetCallbackState::from_definitions(&definitions, &layout)
            .map_err(|e| format!("{e:?}"))?;
        let global = literal(&row["expected_contracts"])?;
        let expected_owners = literal(&row["expected_owners"])?;
        let ErlangTerm::List {
            items: owner_items, ..
        } = expected_owners.term()
        else {
            return Err("expected owners".into());
        };
        let mut owners_matched = true;
        for (index, &owner) in owner_items.iter().enumerate() {
            let ErlangTerm::Tuple(fields) = &expected_owners.arena().terms[owner.0] else {
                unreachable!()
            };
            let expected = RawFormTerm::new(expected_owners.arena().clone(), fields[1])?;
            owners_matched &= matches_map(
                definitions.contracts.owner_entries(ModuleId(index)),
                &expected,
            )?;
        }
        let expected_rows = literal(&row["expected_callback_rows"])?;
        let ErlangTerm::List {
            items: expected_items,
            ..
        } = expected_rows.term()
        else {
            return Err("expected callback rows".into());
        };
        let actual_rows = callbacks.rows();
        let mut rows_matched = actual_rows.len() == expected_items.len();
        for (actual, &expected) in actual_rows.iter().zip(expected_items) {
            let raw = actual.callback_row();
            rows_matched &=
                compare_map_keys(raw.arena(), raw.id(), expected_rows.arena(), expected)?.is_eq();
        }
        let groups = callbacks
            .persistence_groups()
            .map_err(|e| format!("{e:?}"))?
            .raw();
        let matched = matches_map(definitions.contracts.raw_entries(), &global)?
            && owners_matched
            && rows_matched
            && equals(&groups, &row["expected_groups"])?;
        println!(
            "{}",
            json!({"id":row["id"],"matched":matched,"final_rows":definitions.contracts.len(),"writes":definitions.callbacks.write_events().count()})
        );
    }
    Ok(())
}
