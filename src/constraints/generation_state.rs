use super::*;
use crate::constraints::constraint_capture::{self as capture, ConstraintCaptures};
impl Generator<'_> {
    pub(super) fn lookup_record(
        &mut self,
        module: ModuleId,
        tag: &str,
        arity: usize,
    ) -> Result<Option<Type>> {
        if !self.state.record_cache.contains(&module) {
            if let Some(inputs) = self.capture_inputs {
                inputs
                    .records
                    .module_table(module)
                    .map_err(GenerationError::Boundary)?;
            }
            self.state.record_cache.insert(0, module);
        }
        Ok(self.records.get(&(module, tag.to_owned(), arity)).cloned())
    }
    pub(super) fn validate_final_capture_metadata(&mut self) -> Result<()> {
        use crate::constraints::{closure_order::ClosureOrderKey, normalize::NativeConstraint};
        let inputs = self.capture_inputs.ok_or_else(|| {
            GenerationError::Boundary("source phase capture inputs absent".into())
        })?;
        let mut headers = Vec::new();
        inputs.runtime.append(&mut headers);
        // Conversion errors originally surface after all structural capture checks.
        let mut deferred = TermOrderArena::captures_error(&headers);
        for index in 0..self.arena.len() {
            let node = self
                .arena
                .node(NativeConstraintId(index))
                .ok_or_else(|| GenerationError::Boundary("constraint arena gap".into()))?;
            match node {
                NativeConstraint::Atomic {
                    right: NativeOperand::Derived(id),
                    ..
                } => {
                    let (key, _, _) = self.arena.derived_capture(*id).ok_or_else(|| {
                        GenerationError::Boundary("foreign derived capture".into())
                    })?;
                    if deferred.is_none() {
                        deferred = match key {
                            ClosureOrderKey::Local(value) if value.arity > 255 => {
                                Some("capture local function arity outside native range".into())
                            }
                            ClosureOrderKey::External { arity, .. } if *arity > 255 => {
                                Some("capture external function arity outside native range".into())
                            }
                            _ => None,
                        };
                    }
                }
                NativeConstraint::Group { children, .. } => {
                    if children.iter().any(|id| id.0 >= index) {
                        return Err(GenerationError::Boundary(
                            "non-postorder constraint child".into(),
                        ));
                    }
                }
                _ => {}
            }
        }
        for id in self
            .state
            .context
            .iter()
            .rev()
            .chain(self.state.bodies.values())
        {
            if id.0 >= self.arena.len() {
                return Err(GenerationError::Boundary(
                    "foreign constraint capture".into(),
                ));
            }
        }
        if let (true, Some(store)) = (crate::lattice::verify_fast_paths(), inputs.core_captures) {
            let fast = (|| -> std::result::Result<_, String> {
                let mut first = None;
                for (module, variable, _) in &self.phase_entries {
                    let error = store.selected_variable_error(*module, *variable)?;
                    first = first.or(error);
                }
                Ok(first)
            })();
            let full = (|| -> std::result::Result<_, String> {
                let mut names = Vec::new();
                for (module, variable, _) in &self.phase_entries {
                    store
                        .get(*module)?
                        .append_selected(&[*variable], &mut names)?;
                }
                Ok(TermOrderArena::from_captures(&names).err())
            })();
            assert_eq!(fast, full, "selected capture validation fast path diverged");
        }
        let mut names = Vec::new();
        let mut names_error = None;
        for (module, variable, _) in &self.phase_entries {
            self.core.modules[module.0]
                .named_mfa(*variable)
                .map_err(GenerationError::Boundary)?;
            if let Some(store) = inputs.core_captures {
                let error = store
                    .selected_variable_error(*module, *variable)
                    .map_err(GenerationError::Boundary)?;
                names_error = names_error.or(error);
                continue;
            }
            if !self.core_captures.contains_key(module) {
                self.core_captures.insert(
                    *module,
                    crate::core_capture::CoreCaptures::build(self.core, self.labels, *module)
                        .map_err(GenerationError::Boundary)?,
                );
            }
            self.core_captures[module]
                .append_selected(&[*variable], &mut names)
                .map_err(GenerationError::Boundary)?;
        }
        if inputs.core_captures.is_none() {
            names_error = TermOrderArena::from_captures(&names).err();
        }
        if deferred.is_none() {
            deferred = names_error;
        }
        for module in &self.state.record_cache {
            let _ = &self.core.modules[module.0].name;
            let table = inputs
                .records
                .module_table(*module)
                .map_err(GenerationError::Boundary)?;
            if table.len() != 1 {
                return Err(GenerationError::Boundary(
                    "module record capture must have one table".into(),
                ));
            }
        }
        if let Some(error) = deferred {
            return Err(GenerationError::Boundary(error));
        }
        Ok(())
    }
    /// Exact unfinalized #state captured by source-generated functions. Runtime
    /// headers and original prepared tables are supplied by the owning phase.
    pub fn capture_state(&mut self) -> Result<CaptureEnvironment> {
        if exact_captures() {
            return self.capture_state_exact();
        }
        {
            // OTP's BIF constraints are closures over its whole #state, and
            // constraint ordering compares those closures. Reproducing that
            // snapshot per BIF call is costly and does not change warnings or
            // signatures (checked against OTP on Phoenix's 1,082 PLT modules),
            // so a constant stands in for the state by default.
            let mut terms = Vec::new();
            let value = push(&mut terms, CaptureTerm::Integer("0".into()));
            let root = capture::proper_list(&mut terms, vec![value]);
            return CaptureEnvironment::new(
                Arc::new(TermOrderArena::from_captures(&terms).map_err(GenerationError::Boundary)?),
                root,
            )
            .map_err(GenerationError::Boundary);
        }
    }
    /// OTP's complete #state as captured by its BIF constraint closures.
    pub fn capture_state_exact(&mut self) -> Result<CaptureEnvironment> {
        let inputs = self.capture_inputs.ok_or_else(|| {
            GenerationError::Boundary("source phase capture inputs absent".into())
        })?;
        let mut terms = Vec::new();
        let [callgraph, cserver, plt] = inputs.runtime.append(&mut terms);
        let constraints = ConstraintCaptures::append(&self.arena, &mut terms)
            .map_err(GenerationError::Boundary)?;
        let cs = self
            .state
            .context
            .iter()
            .rev()
            .map(|id| constraints.node(*id))
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(GenerationError::Boundary)?;
        let cs = capture::proper_list(&mut terms, cs);
        let cmap = self
            .state
            .bodies
            .iter()
            .map(|(key, value)| {
                Ok((
                    capture::type_variable(&mut terms, key.variable()),
                    constraints.node(*value)?,
                ))
            })
            .collect::<std::result::Result<Vec<_>, String>>()
            .map_err(GenerationError::Boundary)?;
        let cmap = push(&mut terms, CaptureTerm::Map(cmap));
        let fun_map = self
            .state
            .recursive_variables
            .iter()
            .map(|(key, value)| {
                (
                    capture::type_variable(&mut terms, key),
                    capture::type_variable(&mut terms, value),
                )
            })
            .collect();
        let fun_map = push(&mut terms, CaptureTerm::Map(fun_map));
        let arities = self
            .state
            .parameters
            .iter()
            .map(|(key, value)| {
                (
                    capture::type_variable(&mut terms, key.variable()),
                    push(&mut terms, CaptureTerm::Integer(value.arity.to_string())),
                )
            })
            .collect();
        let arities = push(&mut terms, CaptureTerm::Map(arities));
        let in_match = boolean(&mut terms, self.state.in_match);
        let in_guard = boolean(&mut terms, self.state.in_guard);
        let module = match self.state.module {
            Some(module) => atom(&mut terms, &self.core.modules[module.0].name),
            None => atom(&mut terms, "undefined"),
        };
        let mut name_map = Vec::new();
        let mut mfas = Vec::new();
        for (module, variable, _) in &self.phase_entries {
            let mfa = self.core.modules[module.0]
                .named_mfa(*variable)
                .map_err(GenerationError::Boundary)?;
            let key = mfa_capture(&mut terms, &mfa);
            mfas.push(key);
            let captures = if let Some(store) =
                self.capture_inputs.and_then(|inputs| inputs.core_captures)
            {
                store.get(*module).map_err(GenerationError::Boundary)?
            } else {
                if !self.core_captures.contains_key(module) {
                    self.core_captures.insert(
                        *module,
                        crate::core_capture::CoreCaptures::build(self.core, self.labels, *module)
                            .map_err(GenerationError::Boundary)?,
                    );
                }
                &self.core_captures[module]
            };
            let value = captures
                .append_selected(&[*variable], &mut terms)
                .map_err(GenerationError::Boundary)?[0];
            name_map.push((key, value));
        }
        let name_map = push(&mut terms, CaptureTerm::Map(name_map));
        let next_label = push(
            &mut terms,
            CaptureTerm::Integer(self.state.next_label.to_string()),
        );
        let self_rec = match self.self_recursive {
            Some(label) => capture::typ(
                &mut terms,
                &Type::variable_id(VariableId::Integer(label.0.into())),
            ),
            None => atom(&mut terms, "false"),
        };
        let prop_types = self
            .propagated
            .0
            .iter()
            .map(|(key, value)| {
                (
                    push(&mut terms, CaptureTerm::Integer(key.0.to_string())),
                    capture::typ(&mut terms, value),
                )
            })
            .collect();
        let prop_types = push(&mut terms, CaptureTerm::Map(prop_types));
        let mut mod_records = Vec::new();
        for module in &self.state.record_cache {
            let name = atom(&mut terms, &self.core.modules[module.0].name);
            let table = inputs
                .records
                .module_table(*module)
                .map_err(GenerationError::Boundary)?;
            let values = capture::environment_values(&mut terms, table)
                .map_err(GenerationError::Boundary)?;
            let [value]: [CaptureId; 1] = values.try_into().map_err(|_| {
                GenerationError::Boundary("module record capture must have one table".into())
            })?;
            mod_records.push(push(&mut terms, CaptureTerm::Tuple(vec![name, value])));
        }
        let mod_records = capture::proper_list(&mut terms, mod_records);
        let scc = self
            .state
            .scc
            .iter()
            .map(|key| capture::type_variable(&mut terms, key.variable()))
            .collect();
        let scc = capture::proper_list(&mut terms, scc);
        let mfas = capture::proper_list(&mut terms, mfas);
        let solvers = inputs.solvers.iter().map(|s| atom(&mut terms, s)).collect();
        let solvers = capture::proper_list(&mut terms, solvers);
        let tag = atom(&mut terms, "state");
        let state = push(
            &mut terms,
            CaptureTerm::Tuple(vec![
                tag,
                callgraph,
                cserver,
                cs,
                cmap,
                fun_map,
                arities,
                in_match,
                in_guard,
                module,
                name_map,
                next_label,
                self_rec,
                plt,
                prop_types,
                mod_records,
                scc,
                mfas,
                solvers,
            ]),
        );
        let root = capture::proper_list(&mut terms, vec![state]);
        CaptureEnvironment::new(
            Arc::new(TermOrderArena::from_captures(&terms).map_err(GenerationError::Boundary)?),
            root,
        )
        .map_err(GenerationError::Boundary)
    }
}
/// `RUST_DIALYZER_EXACT_CAPTURE=1` restores OTP's full closure-environment
/// order (used by tests that compare generator states).
pub(super) fn exact_captures() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("RUST_DIALYZER_EXACT_CAPTURE").is_some_and(|v| v == "1"))
}
fn push(terms: &mut Vec<CaptureTerm>, value: CaptureTerm) -> CaptureId {
    let id = CaptureId(terms.len());
    terms.push(value);
    id
}
fn atom(terms: &mut Vec<CaptureTerm>, value: &str) -> CaptureId {
    push(terms, CaptureTerm::Atom(value.to_owned()))
}
fn boolean(terms: &mut Vec<CaptureTerm>, value: bool) -> CaptureId {
    atom(terms, if value { "true" } else { "false" })
}
fn mfa_capture(terms: &mut Vec<CaptureTerm>, mfa: &Mfa) -> CaptureId {
    let module = atom(terms, &mfa.module);
    let function = atom(terms, &mfa.function);
    let arity = push(terms, CaptureTerm::Integer(mfa.arity.to_string()));
    push(terms, CaptureTerm::Tuple(vec![module, function, arity]))
}
