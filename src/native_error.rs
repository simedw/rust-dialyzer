use crate::lattice::{LatticeError, NativeValue, Type};
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NativeTransferError {
    FunctionClause,
    Badarg,
    SystemLimit,
    CaseClauseNil,
    CaseClauseType(Box<Type>),
    BadMatchType(Box<Type>),
    BadMatchBoolean(bool),
    BadMatchNative(Box<NativeValue>),
    BadGenerators(Box<(Vec<Type>, Vec<Type>)>),
    BadGenerator(Box<Type>),
    NativeNonTerminating,
    Detail(String),
}
impl From<LatticeError> for NativeTransferError {
    fn from(error: LatticeError) -> Self {
        match error {
            LatticeError::FunctionClause | LatticeError::OpaqueSentinelSubtraction => {
                Self::FunctionClause
            }
            LatticeError::BadArg => Self::Badarg,
            LatticeError::CaseClause(payload) => Self::CaseClauseType(payload),
            LatticeError::BadMatch(payload) => Self::BadMatchType(payload),
            LatticeError::BadMatchBoolean(payload) => Self::BadMatchBoolean(payload),
            LatticeError::BadMatchNative(payload) => Self::BadMatchNative(payload),
            LatticeError::BadGenerators(payload) => Self::BadGenerators(payload),
            LatticeError::BadGenerator(payload) => Self::BadGenerator(payload),
            LatticeError::NativeNonTerminating => Self::NativeNonTerminating,
            #[cfg(otp_domain = "29")]
            LatticeError::RecordIterationOrderRequired => Self::Detail(
                "native record unification requires an authenticated field iteration order".into()),
        }
    }
}
impl From<String> for NativeTransferError {
    fn from(error: String) -> Self {
        Self::Detail(error)
    }
}
impl From<&str> for NativeTransferError {
    fn from(error: &str) -> Self {
        Self::Detail(error.into())
    }
}
impl std::fmt::Display for NativeTransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for NativeTransferError {}
impl NativeTransferError {
    pub fn report(&self) -> serde_json::Value {
        use serde_json::json;
        match self {
            Self::FunctionClause => json!({"kind":"error","class":"function_clause"}),
            Self::Badarg => json!({"kind":"error","class":"badarg"}),
            Self::SystemLimit => json!({"kind":"error","class":"system_limit"}),
            Self::CaseClauseNil => {
                json!({"kind":"error","class":"case_clause","payload":{"kind":"nil"}})
            }
            Self::CaseClauseType(value) => {
                json!({"kind":"error","class":"case_clause","payload":{"kind":"type","value":value}})
            }
            Self::BadMatchType(value) => {
                json!({"kind":"error","class":"badmatch","payload":{"kind":"type","value":value}})
            }
            Self::BadMatchBoolean(value) => {
                json!({"kind":"error","class":"badmatch","payload":{"kind":"boolean","value":value}})
            }
            Self::BadMatchNative(value) => {
                json!({"kind":"error","class":"badmatch","payload":{"kind":"native","value":value.to_value()}})
            }
            Self::BadGenerators(tails) => {
                json!({"kind":"error","class":"bad_generators","payload":{"left":tails.0,"right":tails.1}})
            }
            Self::BadGenerator(value) => {
                json!({"kind":"error","class":"bad_generator","payload":{"kind":"type","value":value}})
            }
            Self::NativeNonTerminating => json!({"kind":"non_terminating"}),
            Self::Detail(error) => json!({"kind":"unsupported","detail":error}),
        }
    }
}
