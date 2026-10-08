mod expand_records;
mod expand_substitutions;
mod expand_unqualified_calls;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use firefly_diagnostics::CodeMap;
use firefly_pass::Pass;
use firefly_intern::symbols;
use firefly_syntax_base::FunctionName;

use crate::ast;

use self::expand_records::ExpandRecords;
use self::expand_substitutions::ExpandSubstitutions;
use self::expand_unqualified_calls::ExpandUnqualifiedCalls;

pub struct CanonicalizeSyntax {
    codemap: Arc<CodeMap>,
}
impl CanonicalizeSyntax {
    pub fn new(codemap: Arc<CodeMap>) -> Self {
        Self { codemap }
    }
}
impl Pass for CanonicalizeSyntax {
    type Input<'a> = ast::Module;
    type Output<'a> = ast::Module;

    fn run<'a>(&mut self, mut module: Self::Input<'a>) -> anyhow::Result<Self::Output<'a>> {
        let mut functions = BTreeMap::new();
        let locals: BTreeSet<_> = module.functions.keys().cloned().collect();
        let record_info_shadowed = locals.contains(&FunctionName::new_local(symbols::RecordInfo, 2));
        while let Some((key, mut function)) = module.functions.pop_first() {
            // Prepare function for translation to CST
            let mut pipeline = ExpandRecords::new(&module)
                .with_record_info_shadowing(record_info_shadowed)
                .chain(ExpandUnqualifiedCalls::new(&module, &locals))
                .chain(ExpandSubstitutions::new(module.name, &self.codemap));
            pipeline.run(&mut function)?;

            functions.insert(key, function);
        }

        module.functions = functions;

        Ok(module)
    }
}
