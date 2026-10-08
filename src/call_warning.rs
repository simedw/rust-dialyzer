use crate::{
    binder::WarningInfo, lattice::NativeValue, program::NodeRef, warning_payload::NativeWarning,
};
/// Source location is retained for the controller; equality follows native warning terms.
#[derive(Clone, Debug)]
pub struct CallWarning {
    pub source: NodeRef,
    pub native: NativeWarning,
}
impl CallWarning {
    pub fn new(source: NodeRef, category: &str, info: &WarningInfo, message: &NativeValue) -> Self {
        Self {
            source,
            native: NativeWarning::from_value(category, info, message),
        }
    }
    pub fn native_warning(&self) -> NativeWarning {
        self.native.clone()
    }
}
impl PartialEq for CallWarning {
    fn eq(&self, other: &Self) -> bool {
        self.native == other.native
    }
}
impl Eq for CallWarning {}
