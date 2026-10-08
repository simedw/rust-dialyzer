use rust_dialyzer_poc::native_runtime_hash::{NativeAtomLayout, NativeHashKey};
use rust_dialyzer_poc::{
    core::{ErlangTerm, ErlangTerms, TermId},
    external_term::{Limits, decode},
    type_exports::{compact_arena, term_to_value},
};
use serde_json::{Value, json};
use std::collections::{BTreeMap, BTreeSet};

fn chunk<'a>(bytes: &'a [u8], wanted: &[u8; 4]) -> Result<&'a [u8], String> {
    if bytes.len() < 12 || &bytes[..4] != b"FOR1" || &bytes[8..12] != b"BEAM" {
        return Err("invalid BEAM header".into());
    }
    let declared = u32::from_be_bytes(bytes[4..8].try_into().unwrap()) as usize;
    if declared.checked_add(8) != Some(bytes.len()) {
        return Err("BEAM container size mismatch".into());
    }
    let mut at: usize = 12;
    let mut found = None;
    while at < bytes.len() {
        let header = bytes
            .get(at..at.checked_add(8).ok_or("offset overflow")?)
            .ok_or("truncated chunk header")?;
        let size = u32::from_be_bytes(header[4..8].try_into().unwrap()) as usize;
        let start = at + 8;
        let end = start.checked_add(size).ok_or("chunk overflow")?;
        let data = bytes.get(start..end).ok_or("truncated chunk")?;
        let padding = (4 - size % 4) % 4;
        at = end.checked_add(padding).ok_or("padding overflow")?;
        if at > bytes.len() {
            return Err("truncated padding".into());
        }
        if &header[..4] == wanted {
            if found.replace(data).is_some() {
                return Err("duplicate debug chunk".into());
            }
        }
    }
    found.ok_or("BEAM has no debug info".into())
}

fn tuple(arena: &ErlangTerms, id: TermId) -> Result<&[TermId], String> {
    match &arena.terms[id.0] {
        ErlangTerm::Tuple(fields) => Ok(fields),
        _ => Err("expected tuple".into()),
    }
}
fn atom(arena: &ErlangTerms, id: TermId) -> Result<&str, String> {
    match &arena.terms[id.0] {
        ErlangTerm::Atom(name) => Ok(name),
        _ => Err("expected atom".into()),
    }
}
fn list(arena: &ErlangTerms, id: TermId) -> Result<Vec<TermId>, String> {
    list_terms(&arena.terms, id)
}
fn list_terms(terms: &[ErlangTerm], mut id: TermId) -> Result<Vec<TermId>, String> {
    let mut result = Vec::new();
    loop {
        match &terms[id.0] {
            ErlangTerm::Nil => return Ok(result),
            ErlangTerm::List { items, tail } => {
                result.extend_from_slice(items);
                id = *tail;
            }
            _ => return Err("expected proper list".into()),
        }
    }
}
fn append(terms: &mut Vec<ErlangTerm>, value: ErlangTerm) -> usize {
    let id = terms.len();
    terms.push(value);
    id
}
fn source_location(arena: &ErlangTerms, id: TermId) -> Result<(u32, u32), String> {
    match &arena.terms[id.0] {
        ErlangTerm::Integer(line) => Ok((line.parse().map_err(|_| "invalid source line")?, 1)),
        ErlangTerm::Tuple(fields) if fields.len() == 2 => {
            let integer = |id: TermId| -> Result<u32, String> {
                match &arena.terms[id.0] {
                    ErlangTerm::Integer(value) => value
                        .parse()
                        .map_err(|_| "invalid source coordinate".into()),
                    _ => Err("source coordinate is not an integer".into()),
                }
            };
            Ok((integer(fields[0])?, integer(fields[1])?))
        }
        ErlangTerm::List { .. } | ErlangTerm::Nil => {
            for item in list(arena, id)? {
                let fields = tuple(arena, item)?;
                if fields.len() == 2 && atom(arena, fields[0]).ok() == Some("location") {
                    return source_location(arena, fields[1]);
                }
            }
            Err("source annotation lacks location".into())
        }
        _ => Err("unsupported source annotation".into()),
    }
}
fn source_string(arena: &ErlangTerms, id: TermId) -> Result<String, String> {
    list(arena, id)?
        .iter()
        .map(|id| match &arena.terms[id.0] {
            ErlangTerm::Integer(value) => value
                .parse::<u32>()
                .ok()
                .and_then(char::from_u32)
                .ok_or("invalid file character".into()),
            _ => Err("file name is not a character list".into()),
        })
        .collect()
}
fn source_sidecar(
    arena: &ErlangTerms,
    forms: TermId,
) -> Result<(BTreeSet<String>, Vec<SourceOrigin>), String> {
    let mut variables = BTreeSet::new();
    let mut origins = Vec::new();
    let mut file = String::new();
    for form in list(arena, forms)? {
        let fields = match &arena.terms[form.0] {
            ErlangTerm::Tuple(fields) => fields,
            _ => continue,
        };
        if fields.len() == 4
            && atom(arena, fields[0]).ok() == Some("attribute")
            && atom(arena, fields[2]).ok() == Some("file")
        {
            file = source_string(arena, tuple(arena, fields[3])?[0])?;
        }
        if fields.is_empty() || atom(arena, fields[0]).ok() != Some("function") {
            continue;
        }
        let mut work = vec![form];
        while let Some(id) = work.pop() {
            match &arena.terms[id.0] {
                ErlangTerm::Tuple(items) => {
                    if items.len() == 3 && atom(arena, items[0]).ok() == Some("var") {
                        variables.insert(atom(arena, items[2])?.to_owned());
                    }
                    if items.len() == 4
                        && matches!(
                            atom(arena, items[0]).ok(),
                            Some("map_field_exact" | "map_field_assoc")
                        )
                    {
                        let kind = atom(arena, items[0])?;
                        let (line, column) = source_location(arena, items[1])?;
                        let key = tuple(arena, items[2])?;
                        if key.len() < 2 {
                            return Err("map key lacks source annotation".into());
                        }
                        let (key_line, key_column) = source_location(arena, key[1])?;
                        origins.push(SourceOrigin::field(
                            kind,
                            file.clone(),
                            line,
                            column,
                            key_line,
                            key_column,
                        ));
                    }
                    if items.len() == 4 && atom(arena, items[0]).ok() == Some("map") {
                        let (line, column) = source_location(arena, items[1])?;
                        let base = tuple(arena, items[2])?;
                        let (base_line, base_column) = source_location(arena, base[1])?;
                        origins.push(SourceOrigin::MapUpdate {
                            file: file.clone(),
                            line,
                            column,
                            base_line,
                            base_column,
                        });
                    }
                    work.extend(items);
                }
                ErlangTerm::List { items, tail } => {
                    work.extend(items);
                    work.push(*tail);
                }
                ErlangTerm::Map(_) => return Err("source metadata maps require provenance".into()),
                _ => {}
            }
        }
    }
    Ok((variables, origins))
}
fn location(terms: &[ErlangTerm], annotation: usize) -> Result<usize, String> {
    match &terms[annotation] {
        ErlangTerm::Integer(_) | ErlangTerm::Tuple(_) => Ok(annotation),
        ErlangTerm::List { items, .. } => {
            for field in items {
                let ErlangTerm::Tuple(pair) = &terms[field.0] else {
                    return Err("invalid annotation pair".into());
                };
                if pair.len() == 2
                    && matches!(&terms[pair[0].0],ErlangTerm::Atom(a) if a=="location")
                {
                    return Ok(pair[1].0);
                }
            }
            Err("source annotation lacks location".into())
        }
        _ => Err("unsupported source annotation".into()),
    }
}
fn wire_items(terms: &[ErlangTerm], id: usize, kind: &str) -> Result<Vec<usize>, String> {
    match (&terms[id], kind) {
        (ErlangTerm::Nil, "list") => Ok(vec![]),
        (ErlangTerm::Tuple(items), "tuple") => Ok(items.iter().map(|i| i.0).collect()),
        (ErlangTerm::List { items, tail }, "list") => {
            let mut out = items.iter().map(|i| i.0).collect::<Vec<_>>();
            out.extend(wire_items(terms, tail.0, "list")?);
            Ok(out)
        }
        _ => Err(format!("bounded contract expected {kind}")),
    }
}
fn text(terms: &[ErlangTerm], id: usize) -> Option<&str> {
    match &terms[id] {
        ErlangTerm::Atom(v) | ErlangTerm::Integer(v) => Some(v),
        _ => None,
    }
}
fn constraint_variable(terms: &[ErlangTerm], mut id: usize) -> Result<usize, String> {
    loop {
        let fields = wire_items(terms, id, "tuple")?;
        if fields.len() != 3 {
            return Err("invalid bounded contract variable".into());
        }
        match text(terms, fields[0]) {
            Some("var") if text(terms, fields[2]).is_some_and(|v| v != "_") => return Ok(fields[2]),
            Some("ann_type") => {
                let pair = wire_items(terms, fields[2], "list")?;
                if pair.len() != 2 {
                    return Err("invalid annotated contract variable".into());
                }
                id = pair[1];
            }
            _ => return Err("unresolved bounded contract variable".into()),
        }
    }
}
fn contract_variable_maps(
    terms: &mut Vec<ErlangTerm>,
    layout: Option<&NativeAtomLayout>,
) -> Result<(BTreeMap<TermId, TermId>, BTreeMap<TermId, Vec<usize>>), String> {
    let original_count = terms.len();
    let nil_atom = terms
        .iter()
        .position(|t| matches!(t,ErlangTerm::Atom(v) if v=="nil"))
        .unwrap_or_else(|| append(terms, ErlangTerm::Atom("nil".into())));
    let mut maps = BTreeMap::new();
    let mut orders = BTreeMap::new();
    for form in 0..original_count {
        if !matches!(&terms[form], ErlangTerm::Tuple(_)) {
            continue;
        }
        let fields = wire_items(terms, form, "tuple")?;
        if fields.len() != 4
            || text(terms, fields[0]) != Some("type")
            || text(terms, fields[2]) != Some("bounded_fun")
        {
            continue;
        }
        let args = wire_items(terms, fields[3], "list")?;
        if args.len() != 2 {
            return Err("invalid bounded_fun arguments".into());
        }
        let mut variables = BTreeMap::new();
        for constraint in wire_items(terms, args[1], "list")? {
            let fields = wire_items(terms, constraint, "tuple")?;
            if fields.len() != 4
                || text(terms, fields[0]) != Some("type")
                || text(terms, fields[2]) != Some("constraint")
            {
                return Err("invalid bounded contract constraint".into());
            }
            let args = wire_items(terms, fields[3], "list")?;
            if args.len() != 2 {
                return Err("invalid bounded constraint arguments".into());
            }
            let operator = wire_items(terms, args[0], "tuple")?;
            if operator.len() != 3
                || text(terms, operator[0]) != Some("atom")
                || text(terms, operator[2]) != Some("is_subtype")
            {
                return Err("unsupported bounded constraint operator".into());
            }
            let pair = wire_items(terms, args[1], "list")?;
            if pair.len() != 2 {
                return Err("invalid bounded subtype operands".into());
            }
            let key = constraint_variable(terms, pair[0])?;
            let value = text(terms, key).ok_or("invalid bounded variable name")?;
            let literal = if matches!(&terms[key], ErlangTerm::Atom(_)) {
                NativeHashKey::Atom(value.into())
            } else {
                NativeHashKey::Integer(
                    value
                        .parse()
                        .map_err(|_| "invalid bounded integer variable")?,
                )
            };
            variables.insert(literal, key);
        }
        let ordered = if variables.is_empty() {
            vec![]
        } else {
            rust_dialyzer_poc::native_map_order::ordered_keys(
                layout.ok_or("bounded contracts require compiler atom layout")?,
                variables.keys().cloned(),
            )
            .map_err(|e| e.to_string())?
        };
        let entries = ordered
            .iter()
            .map(|key| (TermId(variables[key]), TermId(nil_atom)))
            .collect::<Vec<_>>();
        let mut order = (0..entries.len()).collect::<Vec<_>>();
        if entries.len() > 32 {
            order.reverse();
        }
        let map = TermId(append(terms, ErlangTerm::Map(entries)));
        maps.insert(TermId(form), map);
        orders.insert(map, order);
    }
    Ok((maps, orders))
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum SourceOrigin {
    #[serde(rename = "map_field_exact")]
    MapFieldExact {
        file: String,
        line: u32,
        column: u32,
        key_line: u32,
        key_column: u32,
    },
    #[serde(rename = "map_field_assoc")]
    MapFieldAssoc {
        file: String,
        line: u32,
        column: u32,
        key_line: u32,
        key_column: u32,
    },
    #[serde(rename = "map_update")]
    MapUpdate {
        file: String,
        line: u32,
        column: u32,
        base_line: u32,
        base_column: u32,
    },
}
impl SourceOrigin {
    fn field(
        kind: &str,
        file: String,
        line: u32,
        column: u32,
        key_line: u32,
        key_column: u32,
    ) -> Self {
        if kind == "map_field_exact" {
            Self::MapFieldExact {
                file,
                line,
                column,
                key_line,
                key_column,
            }
        } else {
            Self::MapFieldAssoc {
                file,
                line,
                column,
                key_line,
                key_column,
            }
        }
    }
    pub fn position(&self) -> (u32, u32) {
        match self {
            Self::MapFieldExact { line, column, .. }
            | Self::MapFieldAssoc { line, column, .. }
            | Self::MapUpdate { line, column, .. } => (*line, *column),
        }
    }
    pub fn matches_update(&self, file: &str, line: usize, column: usize) -> bool {
        matches!(self,Self::MapUpdate{file:f,base_line,base_column,..} if f==file&&*base_line as usize==line&&*base_column as usize==column)
    }
    pub fn matches_field(&self, kind: &str, file: &str, line: usize, column: usize) -> bool {
        match self {
            Self::MapFieldExact {
                file: f,
                key_line,
                key_column,
                ..
            } if kind == "map_field_exact" => {
                f == file && *key_line as usize == line && *key_column as usize == column
            }
            Self::MapFieldAssoc {
                file: f,
                key_line,
                key_column,
                ..
            } if kind == "map_field_assoc" => {
                f == file && *key_line as usize == line && *key_column as usize == column
            }
            _ => false,
        }
    }
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RawAttribute {
    pub name: String,
    pub value: TermId,
    pub annotations: TermId,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SeedProvenance {
    pub backend: String,
    pub input: String,
    pub decoder: String,
    pub bounded_fun_supported: bool,
    pub atom_layout_scope: Option<String>,
}
#[derive(Clone, Debug)]
pub struct PreparedCoreSeed {
    pub name: String,
    pub terms: Vec<ErlangTerm>,
    pub source_forms: TermId,
    pub attributes: Vec<RawAttribute>,
    pub contract_variable_maps: BTreeMap<TermId, TermId>,
    pub map_iterator_orders: BTreeMap<TermId, Vec<usize>>,
    pub source_variables: BTreeSet<String>,
    pub source_origins: Vec<SourceOrigin>,
    pub source_column_offset: usize,
    pub provenance: Option<SeedProvenance>,
}
impl PreparedCoreSeed {
    pub fn to_wire_value(&self) -> Value {
        let terms = self
            .terms
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut v = term_to_value(t, |id| id.0);
                if matches!(t, ErlangTerm::Map(_)) {
                    v["iterator_order"] = json!(self.map_iterator_orders.get(&TermId(i)));
                }
                v
            })
            .collect::<Vec<_>>();
        json!({"name":self.name,"terms":terms,"source_forms":self.source_forms.0,"attributes":self.attributes,
            "contract_variable_maps":self.contract_variable_maps.iter().map(|(k,v)|[k.0,v.0]).collect::<Vec<_>>(),
            "source_variables":self.source_variables,"source_origins":self.source_origins,"provenance":self.provenance})
    }
    pub fn from_wire_value(raw: &Value) -> Result<Self, String> {
        let terms = raw
            .get("terms")
            .and_then(Value::as_array)
            .ok_or("seed.terms missing")?;
        let root = raw["source_forms"]
            .as_u64()
            .ok_or("seed.source_forms missing")? as usize;
        if root >= terms.len() {
            return Err("source_forms outside terms".into());
        }
        let attributes = raw.get("attributes").and_then(Value::as_array).ok_or(
            "seed.attributes missing: need raw-form attributes independent of oracle Core",
        )?;
        let arena = ErlangTerms::parse(&json!({"terms":terms,"root":root}))?;
        let attributes =
            serde_json::from_value::<Vec<RawAttribute>>(Value::Array(attributes.clone()))
                .map_err(|e| format!("seed.attributes: {e}"))?;
        for attr in &attributes {
            if attr.value.0 >= arena.terms.len() || attr.annotations.0 >= arena.terms.len() {
                return Err("attribute term outside seed arena".into());
            }
        }
        let mut orders = BTreeMap::new();
        for (i, t) in arena.terms.iter().enumerate() {
            if let ErlangTerm::Map(entries) = t {
                let order = terms[i]["iterator_order"]
                    .as_array()
                    .ok_or("seed map iterator provenance missing")?
                    .iter()
                    .map(|v| {
                        v.as_u64()
                            .map(|i| i as usize)
                            .ok_or("invalid seed map iterator index")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                let mut sorted = order.clone();
                sorted.sort();
                if sorted != (0..entries.len()).collect::<Vec<_>>() {
                    return Err("invalid seed map iterator permutation".into());
                }
                orders.insert(TermId(i), order);
            }
        }
        let mut maps = BTreeMap::new();
        if let Some(raw) = raw.get("contract_variable_maps") {
            for pair in raw
                .as_array()
                .ok_or("seed.contract_variable_maps: expected array")?
            {
                let pair = pair
                    .as_array()
                    .filter(|p| p.len() == 2)
                    .ok_or("seed contract map: expected pair")?;
                let k = pair[0].as_u64().ok_or("invalid seed contract form")? as usize;
                let v = pair[1].as_u64().ok_or("invalid seed contract map")? as usize;
                if k >= arena.terms.len() || v >= arena.terms.len() {
                    return Err("seed contract map outside terms".into());
                }
                if maps.insert(TermId(k), TermId(v)).is_some() {
                    return Err("duplicate seed contract map".into());
                }
            }
        }
        let seed = Self {
            name: raw["name"].as_str().unwrap_or_default().into(),
            terms: arena.terms,
            source_forms: arena.root,
            attributes,
            contract_variable_maps: maps,
            map_iterator_orders: orders,
            source_variables: serde_json::from_value(
                raw.get("source_variables").cloned().unwrap_or(json!([])),
            )
            .map_err(|e| format!("seed.source_variables: {e}"))?,
            source_origins: serde_json::from_value(
                raw.get("source_origins").cloned().unwrap_or(json!([])),
            )
            .map_err(|e| format!("seed.source_origins: {e}"))?,
            source_column_offset: raw["source_column_offset"].as_u64().unwrap_or(0) as usize,
            provenance: raw
                .get("provenance")
                .filter(|v| !v.is_null())
                .map(|v| {
                    serde_json::from_value(v.clone()).map_err(|e| format!("seed.provenance: {e}"))
                })
                .transpose()?,
        };
        list_terms(&seed.terms, seed.source_forms)
            .map_err(|e| format!("seed.source_forms: {e}"))?;
        for attr in &seed.attributes {
            list_terms(&seed.terms, attr.annotations)
                .map_err(|e| format!("seed.attribute.annotations: {e}"))?;
        }
        for (form, map) in &seed.contract_variable_maps {
            let ErlangTerm::Tuple(fields) = &seed.terms[form.0] else {
                return Err("seed contract map: expected bounded_fun AST".into());
            };
            if fields.len() != 4
                || text(&seed.terms, fields[0].0) != Some("type")
                || text(&seed.terms, fields[2].0) != Some("bounded_fun")
            {
                return Err("seed contract map: expected bounded_fun AST".into());
            }
            let ErlangTerm::Map(pairs) = &seed.terms[map.0] else {
                return Err("seed contract map: expected compiler variable map".into());
            };
            if pairs.iter().any(|(k, v)| {
                !matches!(&seed.terms[k.0],ErlangTerm::Atom(a) if a!="_")
                    && !matches!(&seed.terms[k.0], ErlangTerm::Integer(_))
                    || !matches!(&seed.terms[v.0],ErlangTerm::Atom(a) if a=="nil")
            }) {
                return Err("seed contract map: invalid variable/nil entries".into());
            }
        }
        Ok(seed)
    }
}

pub struct PreparedBeam {
    pub seed: PreparedCoreSeed,
    pub forms: ErlangTerms,
    pub translated: bool,
}

pub fn prepare(
    path: &str,
    atom_layout: Option<&NativeAtomLayout>,
    compiler: elixir_frontend::CompilerVersion,
) -> Result<PreparedBeam, String> {
    let mut timings = rust_dialyzer_poc::phase_timings::PhaseTimings::new("beam_seed");
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > Limits::default().bytes as u64 {
        return Err("BEAM exceeds byte limit".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let arena = decode(chunk(&bytes, b"Dbgi")?, Limits::default()).map_err(|e| e.to_string())?;
    timings.mark("beam_read_and_etf_decode");
    let debug = tuple(&arena, arena.root)?;
    if debug.len() != 3 || atom(&arena, debug[0])? != "debug_info_v1" {
        return Err("unsupported debug version".into());
    }
    let backend = atom(&arena, debug[1])?.to_owned();
    let mut forms_arena = if backend == "elixir_erl" {
        elixir_frontend::translate_with_version(&arena, debug[2], compiler)?
    } else if backend == "erl_abstract_code" {
        let metadata = tuple(&arena, debug[2])?;
        if metadata.len() != 2 {
            return Err("invalid Erlang debug metadata".into());
        }
        let mut output = arena.clone();
        output.root = metadata[0];
        output
    } else {
        return Err(format!("unsupported native backend: {backend}"));
    };
    timings.mark("elixir_translation");
    let arena = &mut forms_arena;
    let forms = arena.root;
    list(&arena, forms)?;
    let (source_variables, source_origins) = source_sidecar(&arena, forms)?;
    timings.mark("source_origins_and_metadata");
    let compact = compact_arena(arena);
    let source_forms = compact.root;
    let mut terms = compact.terms;
    if terms.iter().any(|t| matches!(t, ErlangTerm::Map(_))) {
        return Err("metadata maps require runtime-order provenance".into());
    }
    let (contract_maps, map_iterator_orders) = contract_variable_maps(&mut terms, atom_layout)?;
    let nil = terms
        .iter()
        .position(|t| matches!(t, ErlangTerm::Nil))
        .ok_or("source forms have no nil")?;
    let ErlangTerm::List {
        items: form_ids, ..
    } = &terms[source_forms.0]
    else {
        return Err("forms must have flat list representation".into());
    };
    let form_ids = form_ids.clone();
    let mut attributes = vec![];
    let mut name = None;
    for id in form_ids {
        let ErlangTerm::Tuple(fields) = &terms[id.0] else {
            continue;
        };
        if fields.len() != 4 {
            continue;
        }
        let ids = fields.iter().map(|id| id.0).collect::<Vec<_>>();
        if text(&terms, ids[0]) != Some("attribute") {
            continue;
        }
        let key = match &terms[ids[2]] {
            ErlangTerm::Atom(v) => v.clone(),
            _ => return Err("attribute key is not atom".into()),
        };
        if key == "module" {
            name = text(&terms, ids[3]).map(str::to_owned);
        }
        if matches!(key.as_str(), "module" | "export" | "import" | "nifs") {
            continue;
        }
        let loc = location(&terms, ids[1])?;
        let annotations = append(
            &mut terms,
            ErlangTerm::List {
                items: vec![TermId(loc)],
                tail: TermId(nil),
            },
        );
        let value = if matches!(&terms[ids[3]], ErlangTerm::List { .. } | ErlangTerm::Nil) {
            ids[3]
        } else {
            append(
                &mut terms,
                ErlangTerm::List {
                    items: vec![TermId(ids[3])],
                    tail: TermId(nil),
                },
            )
        };
        attributes.push(RawAttribute {
            name: key,
            value: TermId(value),
            annotations: TermId(annotations),
        });
    }
    let seed = PreparedCoreSeed {
        name: name.ok_or("module attribute missing")?,
        terms,
        source_forms,
        attributes,
        contract_variable_maps: contract_maps,
        map_iterator_orders,
        source_variables,
        source_origins,
        source_column_offset: 0,
        provenance: Some(SeedProvenance {
            backend: backend.clone(),
            input: path.into(),
            decoder: "existing bounded Rust ETF arena".into(),
            bounded_fun_supported: true,
            atom_layout_scope: atom_layout.map(|layout| layout.scope.clone()),
        }),
    };
    timings.mark("wire_and_attributes");
    Ok(PreparedBeam {
        seed,
        forms: forms_arena,
        translated: backend == "elixir_erl",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn append(terms: &mut Vec<Value>, value: Value) -> usize {
        let id = terms.len();
        terms.push(value);
        id
    }
    fn contract_variable_maps(
        terms: &mut Vec<Value>,
        layout: Option<&NativeAtomLayout>,
    ) -> Result<Vec<Value>, String> {
        let mut arena = ErlangTerms::parse(&json!({"terms":terms,"root":0}))?;
        let (maps, orders) = super::contract_variable_maps(&mut arena.terms, layout)?;
        *terms = arena
            .terms
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let mut v = term_to_value(t, |id| id.0);
                if let Some(order) = orders.get(&TermId(i)) {
                    v["iterator_order"] = json!(order);
                }
                v
            })
            .collect();
        Ok(maps.iter().map(|(k, v)| json!([k.0, v.0])).collect())
    }
    struct Builder {
        terms: Vec<Value>,
    }
    impl Builder {
        fn new() -> Self {
            Self {
                terms: vec![json!({"kind":"nil"})],
            }
        }
        fn atom(&mut self, name: &str) -> usize {
            append(&mut self.terms, json!({"kind":"atom","value":name}))
        }
        fn list(&mut self, items: Vec<usize>) -> usize {
            append(
                &mut self.terms,
                json!({"kind":"list","items":items,"tail":0}),
            )
        }
        fn tuple(&mut self, items: Vec<usize>) -> usize {
            append(&mut self.terms, json!({"kind":"tuple","items":items}))
        }
        fn constraint(&mut self, key: usize, annotated: bool) -> usize {
            let var = self.atom("var");
            let mut left = self.tuple(vec![var, 0, key]);
            if annotated {
                let ann = self.atom("ann_type");
                let pair = self.list(vec![left, left]);
                left = self.tuple(vec![ann, 0, pair]);
            }
            let atom = self.atom("atom");
            let op = self.atom("is_subtype");
            let op = self.tuple(vec![atom, 0, op]);
            let pair = self.list(vec![left, 0]);
            let args = self.list(vec![op, pair]);
            let ty = self.atom("type");
            let constraint = self.atom("constraint");
            self.tuple(vec![ty, 0, constraint, args])
        }
        fn bounded(&mut self, constraints: Vec<usize>) -> usize {
            let constraints = self.list(constraints);
            let args = self.list(vec![0, constraints]);
            let ty = self.atom("type");
            let bounded = self.atom("bounded_fun");
            self.tuple(vec![ty, 0, bounded, args])
        }
    }
    fn fixture() -> Value {
        serde_json::from_str(include_str!(
            "../../../reports/real-projects/fixes/contracts/map-order.json"
        ))
        .unwrap()
    }
    #[test]
    fn seed_cli_boundary_validates_terms_provenance_and_annotation_lists() {
        let valid = json!({"name":"fixture","terms":[{"kind":"nil"},{"kind":"atom","value":"fixture"}],"source_forms":0,"attributes":[],"contract_variable_maps":[]});
        assert!(PreparedCoreSeed::from_wire_value(&valid).is_ok());
        for (pointer, value, expected) in [
            (
                "/source_forms",
                json!(1),
                "seed.source_forms: expected proper list",
            ),
            (
                "/contract_variable_maps",
                Value::Null,
                "seed.contract_variable_maps: expected array",
            ),
            (
                "/terms/1",
                json!({"kind":"integer","value":"-0"}),
                "canonical decimal integer",
            ),
            (
                "/attributes",
                json!([{"name":"type","value":1,"annotations":1}]),
                "seed.attribute.annotations: expected proper list",
            ),
            (
                "/contract_variable_maps",
                json!([[1, 1]]),
                "expected bounded_fun AST",
            ),
        ] {
            let mut raw = valid.clone();
            *raw.pointer_mut(pointer).unwrap() = value;
            assert!(
                PreparedCoreSeed::from_wire_value(&raw)
                    .unwrap_err()
                    .contains(expected),
                "{pointer}"
            );
        }
    }
    #[test]
    fn constrained_names_deduplicate_and_keep_map_iterator_provenance() {
        let fixture = fixture();
        let layout = NativeAtomLayout::parse(&fixture["layout"]).unwrap();
        let mut b = Builder::new();
        let mut constraints = vec![];
        for (index, key) in fixture["keys"].as_array().unwrap().iter().enumerate() {
            let key = append(&mut b.terms, key.clone());
            constraints.push(b.constraint(key, index % 2 == 0));
        }
        constraints.push(constraints[0]);
        let form = b.bounded(constraints);
        let maps = contract_variable_maps(&mut b.terms, Some(&layout)).unwrap();
        assert_eq!(maps.len(), 1);
        assert_eq!(maps[0][0], form);
        let map = &b.terms[maps[0][1].as_u64().unwrap() as usize];
        let pairs = map["entries"].as_array().unwrap();
        let keys = pairs
            .iter()
            .map(|pair| b.terms[pair[0].as_u64().unwrap() as usize].clone())
            .collect::<Vec<_>>();
        assert_eq!(json!(keys), fixture["to_list"]);
        let keys = map["iterator_order"]
            .as_array()
            .unwrap()
            .iter()
            .map(|i| keys[i.as_u64().unwrap() as usize].clone())
            .collect::<Vec<_>>();
        assert_eq!(json!(keys), fixture["iterator"]);
        for pair in pairs {
            assert_eq!(b.terms[pair[1].as_u64().unwrap() as usize]["value"], "nil");
        }
    }
    #[test]
    fn flat_map_orders_names_by_runtime_atom_identity() {
        let fixture = fixture();
        let layout = NativeAtomLayout::parse(&fixture["layout"]).unwrap();
        let mut b = Builder::new();
        let mut constraints = vec![];
        for key in fixture["keys"].as_array().unwrap().iter().take(4).rev() {
            let key = append(&mut b.terms, key.clone());
            constraints.push(b.constraint(key, false));
        }
        b.bounded(constraints);
        let maps = contract_variable_maps(&mut b.terms, Some(&layout)).unwrap();
        let map = &b.terms[maps[0][1].as_u64().unwrap() as usize];
        let names = map["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|pair| {
                b.terms[pair[0].as_u64().unwrap() as usize]["value"]
                    .as_str()
                    .unwrap()
            })
            .collect::<Vec<_>>();
        let mut identities = names
            .iter()
            .map(|name| layout.atom_identity(name).unwrap())
            .collect::<Vec<_>>();
        let actual = identities.clone();
        identities.sort();
        assert_eq!(actual, identities);
        assert_eq!(map["iterator_order"], json!([0, 1, 2, 3]));
    }
    #[test]
    fn empty_constraints_need_no_atom_context() {
        let mut b = Builder::new();
        b.bounded(vec![]);
        let maps = contract_variable_maps(&mut b.terms, None).unwrap();
        assert_eq!(
            b.terms[maps[0][1].as_u64().unwrap() as usize]["entries"],
            json!([])
        );
    }
    #[test]
    fn malformed_and_anonymous_constraints_reject() {
        let mut b = Builder::new();
        let key = b.atom("_");
        let constraint = b.constraint(key, false);
        b.bounded(vec![constraint]);
        assert!(
            contract_variable_maps(&mut b.terms, None)
                .unwrap_err()
                .contains("unresolved")
        );
        let mut b = Builder::new();
        let bad = b.atom("invalid");
        b.bounded(vec![bad]);
        assert!(contract_variable_maps(&mut b.terms, None).is_err());
    }
    #[test]
    fn nonempty_variable_map_requires_matching_atom_context() {
        let mut b = Builder::new();
        let key = b.atom("missing");
        let constraint = b.constraint(key, false);
        b.bounded(vec![constraint]);
        assert!(
            contract_variable_maps(&mut b.terms.clone(), None)
                .unwrap_err()
                .contains("atom layout")
        );
        let layout = NativeAtomLayout::parse(&fixture()["layout"]).unwrap();
        assert!(
            contract_variable_maps(&mut b.terms, Some(&layout))
                .unwrap_err()
                .contains("MissingAtom")
        );
    }
}
