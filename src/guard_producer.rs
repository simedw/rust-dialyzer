use crate::folded_core::FoldedCore;
use crate::{
    binder::BinderState,
    core::{CoreExpr, ErlangTerm},
    core_print::{CorePrinter, PrintError},
    dataflow_nodes::DataflowNodes,
    form_error::FormError,
    native_error::NativeTransferError,
    native_type_operand::NativeTypeOperand,
    patterns::Patterns,
    program::NodeRef,
};
#[derive(Clone, Debug)]
pub enum ProducerError {
    Form(FormError),
    Core(PrintError),
    Boundary(String),
}
impl From<FormError> for ProducerError {
    fn from(value: FormError) -> Self {
        Self::Form(value)
    }
}
impl From<String> for ProducerError {
    fn from(value: String) -> Self {
        Self::Boundary(value)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArgumentRef {
    Original(NodeRef),
    Folded(NodeRef),
}
impl ArgumentRef {
    fn original(self) -> NodeRef {
        match self {
            Self::Original(node) | Self::Folded(node) => node,
        }
    }
}
pub struct GuardProducer<'a, 'b, 'c> {
    pub original: &'a DataflowNodes<'b>,
    pub folded: &'a FoldedCore,
    pub patterns: &'a Patterns<'a, 'c>,
}
impl GuardProducer<'_, '_, '_> {
    fn function_clause<T>() -> Result<T, ProducerError> {
        Err(ProducerError::Form(FormError::Native(
            NativeTransferError::FunctionClause,
        )))
    }
    fn prefix(&self, arg: NodeRef) -> Result<String, ProducerError> {
        let CoreExpr::Var(id) = self.original.expression(arg)? else {
            return Ok(String::new());
        };
        let ErlangTerm::Atom(name) = &self.original.module(arg.module)?.terms[id.0] else {
            return Ok(String::new());
        };
        Ok(
            if name.starts_with('@') || name.starts_with("cor") || name.starts_with("rec") {
                String::new()
            } else {
                format!("{name}::")
            },
        )
    }
    fn one<W>(
        &self,
        arg: ArgumentRef,
        typ: &NativeTypeOperand,
        state: &BinderState<W>,
        fold: bool,
    ) -> Result<String, ProducerError> {
        let original = arg.original();
        let printed = if fold || matches!(arg, ArgumentRef::Folded(_)) {
            self.folded.root(original)?
        } else {
            original
        };
        let module = self.patterns.nodes.module(printed.module)?;
        if matches!(module.nodes[printed.node].expr, CoreExpr::Literal(_)) {
            return CorePrinter {
                patterns: self.patterns,
            }
            .format_cerl(printed)
            .map_err(ProducerError::Core);
        }
        Ok(format!(
            "{}{}",
            self.prefix(original)?,
            typ.to_string_with(&state.presentation_context())?
        ))
    }
    fn sequence<W>(
        &self,
        args: &[ArgumentRef],
        types: &[NativeTypeOperand],
        state: &BinderState<W>,
        fold: bool,
    ) -> Result<String, ProducerError> {
        if args.is_empty() || types.is_empty() {
            return Self::function_clause();
        }
        let mut pieces = Vec::with_capacity(args.len());
        for (arg, typ) in args.iter().zip(types) {
            pieces.push(self.one(*arg, typ, state, fold)?)
        }
        if args.len() != types.len() {
            return Self::function_clause();
        }
        Ok(pieces.join(","))
    }
    pub fn format_args<W>(
        &self,
        args: &[ArgumentRef],
        types: &[NativeTypeOperand],
        state: &BinderState<W>,
    ) -> Result<String, ProducerError> {
        if args.is_empty() && types.is_empty() {
            return Ok("()".into());
        }
        Ok(format!("({})", self.sequence(args, types, state, true)?))
    }
    pub fn format_args_1<W>(
        &self,
        args: &[ArgumentRef],
        types: &[NativeTypeOperand],
        state: &BinderState<W>,
    ) -> Result<String, ProducerError> {
        self.sequence(args, types, state, false)
    }
}
