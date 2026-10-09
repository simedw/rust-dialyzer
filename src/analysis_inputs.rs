use crate::{
    bifs::registry::Mfa,
    body_calls::{ActiveContract, CallWorld},
    call_transfer::Signature,
    compiled_definitions::PreparedDefinitions,
    constraints::generation::{self, LibraryTypes, RecordTableCaptures},
    dependencies::Store,
    lattice::Type,
    program::ModuleId,
    term_order::CaptureEnvironment,
};
use std::{collections::BTreeMap, sync::Arc};

impl<'library> CallWorld<'library> {
    pub fn from_library(library: &'library Store) -> Self {
        Self {
            library: Some(library),
            ..Self::default()
        }
    }
    pub fn named_signature(&self, name: &Mfa) -> Result<Option<Signature>, String> {
        if let Some(signature) = self.named_signatures.get(name) {
            return Ok(Some(signature.clone()));
        }
        self.library
            .map(|library| library.summary(name))
            .transpose()
            .map(|summary| {
                summary.flatten().map(|summary| Signature {
                    arguments: summary.arguments,
                    result: summary.result,
                })
            })
    }
    pub fn active_contract(&self, name: &Mfa) -> Result<Option<ActiveContract>, String> {
        if let Some(contract) = self.active_contracts.get(name) {
            return Ok(Some(contract.clone()));
        }
        self.library
            .and_then(|library| library.contracts.get(name))
            .map(|contract| {
                Ok(ActiveContract {
                    summary: Arc::new(contract.summary()?.clone()),
                    capture: contract.captured_record(),
                })
            })
            .transpose()
    }
}
impl LibraryTypes for CallWorld<'_> {
    fn signature(&self, name: &Mfa) -> Result<Option<generation::CallSignature>, String> {
        Ok(self
            .named_signature(name)?
            .map(|signature| generation::CallSignature {
                arguments: signature.arguments.clone(),
                returns: signature.result.clone(),
            }))
    }
    fn active_contract(&self, name: &Mfa) -> Result<Option<generation::ActiveContract>, String> {
        CallWorld::active_contract(self, name)?
            .map(|contract| generation_contract(&contract))
            .transpose()
    }
}
/// Generation's view of an active contract.
pub fn generation_contract(contract: &ActiveContract) -> Result<generation::ActiveContract, String> {
    generation::ActiveContract::new(
        contract.summary.clone(),
        CaptureEnvironment::literal_roots_lazy(
            contract.capture.arena().clone(),
            vec![contract.capture.id()],
        ),
    )
}

pub struct PreparedRecords {
    pub prototypes: BTreeMap<(ModuleId, String, usize), Type>,
    tables: BTreeMap<ModuleId, CaptureEnvironment>,
}
impl PreparedRecords {
    pub fn for_modules(
        definitions: &PreparedDefinitions,
        modules: impl IntoIterator<Item = ModuleId>,
    ) -> Result<Self, String> {
        let modules = modules
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>();
        let prototypes = definitions
            .records
            .iter()
            .filter(|((module, _, _), _)| modules.contains(module))
            .map(|(key, record)| (key.clone(), record.prototype.clone()))
            .collect();
        let modules = modules.into_iter().collect::<Vec<_>>();
        let tables = crate::ordered_parallel::setup_map(&modules, |&module| {
            let table = definitions
                .module_table(module)
                .ok_or_else(|| format!("prepared module {} has no record table", module.0))?;
            CaptureEnvironment::literal_roots(table.arena(), &[table.root()])
                .map(|capture| (module, capture))
        })
        .into_iter()
        .collect::<Result<_, _>>()?;
        Ok(Self { prototypes, tables })
    }
}
impl RecordTableCaptures for PreparedRecords {
    fn module_table(&self, module: ModuleId) -> Result<&CaptureEnvironment, String> {
        self.tables
            .get(&module)
            .ok_or_else(|| format!("prepared module {} has no record table", module.0))
    }
}
