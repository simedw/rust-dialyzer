use crate::{
    binder::{BinderRecord, BinderState, WarningFunction},
    compiled_definitions::PreparedDefinitions,
    core::{CoreModule, CoreProgram, ErlangTerm, TermId},
    emission::WarningState,
    lattice::PresentationRecordField,
    program::ModuleId,
};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ModuleBinderError {
    ModuleOutsideCompilerInput(ModuleId),
    Filename {
        module: ModuleId,
        term: TermId,
        cause: CompilerFilenameError,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilerFilenameError {
    NonIntegerCharacter,
    InvalidUnicodeCharacter,
    NonStringToken,
    ImproperCharacterList,
}
impl std::fmt::Display for ModuleBinderError {
    fn fmt(&self, out: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(out, "{self:?}")
    }
}
impl std::error::Error for ModuleBinderError {}

impl<W> BinderState<W> {
    pub fn for_module(
        core: &CoreProgram,
        definitions: &PreparedDefinitions,
        module: ModuleId,
        warning_mode: bool,
    ) -> Result<Self, ModuleBinderError> {
        let mut state = Self::for_compiler_module(core, module, warning_mode)?;
        state.records = definitions
            .records
            .range((module, String::new(), 0)..)
            .take_while(|((owner, _, _), _)| *owner == module)
            .map(|((_, tag, arity), record)| {
                (
                    (tag.clone(), *arity),
                    BinderRecord {
                        prototype: record.prototype.clone(),
                        fields: record
                            .field_names
                            .iter()
                            .zip(&record.field_types)
                            .map(|(name, declared_type)| PresentationRecordField {
                                name: name.clone(),
                                declared_type: declared_type.clone(),
                            })
                            .collect(),
                    },
                )
            })
            .collect::<BTreeMap<_, _>>()
            .into();
        #[cfg(otp_domain = "29")]
        {
            // PreparedDefinitions exposes known external prototypes under the
            // target owner, matching OTP's codeserver lookup for that module.
            state.native_records = definitions.native_records.iter().filter(|((owner, _, _), _)| *owner == module).map(|((_, defining_module, name), record)| {
                ((defining_module.clone(), name.clone()), BinderRecord { prototype: record.prototype,
                    fields: record.field_names.iter().zip(&record.field_types).map(|(name, declared_type)| PresentationRecordField { name: name.clone(), declared_type: *declared_type }).collect() })
            }).collect::<BTreeMap<_, _>>().into();
            state.native_record_imports = definitions.native_record_imports.iter().filter(|((owner, _), _)| *owner == module)
                .map(|((_, name), defining_module)| (name.clone(), defining_module.clone())).collect::<BTreeMap<_, _>>().into();
        }
        Ok(state)
    }

    pub fn for_compiler_module(
        core: &CoreProgram,
        module: ModuleId,
        warning_mode: bool,
    ) -> Result<Self, ModuleBinderError> {
        let source = core
            .modules
            .get(module.0)
            .ok_or(ModuleBinderError::ModuleOutsideCompilerInput(module))?;
        let mut translated_files = BTreeMap::new();
        let mut seen = vec![false; source.terms.len()];
        for node in &source.nodes {
            if std::mem::replace(&mut seen[node.annotations.0], true) {
                continue;
            }
            let ErlangTerm::List { items, .. } = &source.terms[node.annotations.0] else {
                continue;
            };
            for item in items {
                let ErlangTerm::Tuple(pair) = &source.terms[item.0] else {
                    continue;
                };
                if pair.len() == 2
                    && matches!(&source.terms[pair[0].0], ErlangTerm::Atom(tag) if tag == "file")
                {
                    // Repeated annotations share the term, so the first parse decides.
                    if let std::collections::btree_map::Entry::Vacant(entry) =
                        translated_files.entry((module, pair[1]))
                    {
                        entry.insert(filename(source, pair[1]).map_err(|cause| {
                            ModuleBinderError::Filename {
                                module,
                                term: pair[1],
                                cause,
                            }
                        })?);
                    }
                }
            }
        }
        Ok(Self {
            module: source.name.as_str().into(),
            current_fun: WarningFunction::Undefined,
            translated_files: translated_files.into(),
            letrec: BTreeMap::new().into(),
            records: BTreeMap::new().into(),
            #[cfg(otp_domain = "29")]
            native_records: BTreeMap::new().into(),
            #[cfg(otp_domain = "29")]
            native_record_imports: BTreeMap::new().into(),
            warnings: WarningState {
                warning_mode,
                warnings: Vec::new(),
            },
        })
    }
}

fn filename(module: &CoreModule, mut id: TermId) -> Result<String, CompilerFilenameError> {
    let mut result = String::new();
    loop {
        match &module.terms[id.0] {
            ErlangTerm::Nil => return Ok(result),
            ErlangTerm::List { items, tail } => {
                for item in items {
                    let ErlangTerm::Integer(codepoint) = &module.terms[item.0] else {
                        return Err(CompilerFilenameError::NonIntegerCharacter);
                    };
                    let character = codepoint
                        .parse::<u32>()
                        .ok()
                        .and_then(char::from_u32)
                        .ok_or(CompilerFilenameError::InvalidUnicodeCharacter)?;
                    result.push(character);
                }
                id = *tail;
            }
            _ => {
                return Err(if result.is_empty() {
                    CompilerFilenameError::NonStringToken
                } else {
                    CompilerFilenameError::ImproperCharacterList
                });
            }
        }
    }
}
