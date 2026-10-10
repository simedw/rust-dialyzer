use crate::labels::CoreLabel;
use std::{cmp::Ordering, num::NonZeroU64};
/// Source/Core labels remain distinct from refolding's negative labels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DataflowKey {
    Core(CoreLabel),
    Synthetic(SyntheticLabel),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SyntheticLabel(NonZeroU64);
impl SyntheticLabel {
    pub fn new(magnitude: u64) -> Result<Self, String> {
        NonZeroU64::new(magnitude)
            .map(Self)
            .ok_or("synthetic label magnitude must be nonzero".into())
    }
    pub fn magnitude(self) -> u64 {
        self.0.get()
    }
}
impl From<CoreLabel> for DataflowKey {
    fn from(label: CoreLabel) -> Self {
        Self::Core(label)
    }
}
impl From<SyntheticLabel> for DataflowKey {
    fn from(label: SyntheticLabel) -> Self {
        Self::Synthetic(label)
    }
}
impl Ord for DataflowKey {
    fn cmp(&self, rhs: &Self) -> Ordering {
        match (self, rhs) {
            (Self::Core(a), Self::Core(b)) => a.cmp(b),
            (Self::Synthetic(a), Self::Synthetic(b)) => b.0.cmp(&a.0),
            (Self::Synthetic(_), Self::Core(_)) => Ordering::Less,
            (Self::Core(_), Self::Synthetic(_)) => Ordering::Greater,
        }
    }
}
impl PartialOrd for DataflowKey {
    fn partial_cmp(&self, rhs: &Self) -> Option<Ordering> {
        Some(self.cmp(rhs))
    }
}
impl DataflowKey {
    pub fn native_integer(self) -> String {
        match self {
            Self::Core(label) => label.0.to_string(),
            Self::Synthetic(label) => format!("-{}", label.magnitude()),
        }
    }
}
/// Compiler-owned allocation metadata; default profile uses relative identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SyntheticAllocationProfile {
    first: NonZeroU64,
    stride: NonZeroU64,
}
impl SyntheticAllocationProfile {
    pub fn new(first: u64, stride: u64) -> Result<Self, String> {
        Ok(Self {
            first: NonZeroU64::new(first).ok_or("synthetic first magnitude must be nonzero")?,
            stride: NonZeroU64::new(stride).ok_or("synthetic stride must be nonzero")?,
        })
    }
    pub fn first(self) -> u64 {
        self.first.get()
    }
    pub fn stride(self) -> u64 {
        self.stride.get()
    }
}
impl Default for SyntheticAllocationProfile {
    fn default() -> Self {
        Self {
            first: NonZeroU64::MIN,
            stride: NonZeroU64::MIN,
        }
    }
}
#[derive(Debug)]
pub struct SyntheticLabels {
    profile: SyntheticAllocationProfile,
    next: Option<NonZeroU64>,
    allocated: u64,
}
impl Default for SyntheticLabels {
    fn default() -> Self {
        Self::with_profile(SyntheticAllocationProfile::default())
    }
}
impl SyntheticLabels {
    pub fn with_profile(profile: SyntheticAllocationProfile) -> Self {
        Self {
            next: Some(profile.first),
            profile,
            allocated: 0,
        }
    }
    pub fn profile(&self) -> SyntheticAllocationProfile {
        self.profile
    }
    pub fn allocations(&self) -> u64 {
        self.allocated
    }
    /// The allocator after `count` further allocations, as if `allocate`
    /// had been called that many times without using the labels.
    pub fn advanced(&self, count: u64) -> Self {
        let next = self.next.and_then(|next| {
            count
                .checked_mul(self.profile.stride.get())
                .and_then(|step| next.get().checked_add(step))
                .and_then(NonZeroU64::new)
        });
        Self {
            profile: self.profile,
            next,
            allocated: self.allocated.saturating_add(count),
        }
    }
    pub fn allocate(&mut self) -> Result<SyntheticLabel, String> {
        let value = self.next.ok_or("synthetic label allocator exhausted")?;
        let allocated = self
            .allocated
            .checked_add(1)
            .ok_or("synthetic allocation count exhausted")?;
        self.next = value
            .get()
            .checked_add(self.profile.stride.get())
            .and_then(NonZeroU64::new);
        self.allocated = allocated;
        Ok(SyntheticLabel(value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiler_profile_is_explicit_and_exhaustion_preserves_last_identity() {
        assert!(SyntheticAllocationProfile::new(0, 64).is_err());
        assert!(SyntheticAllocationProfile::new(2628, 0).is_err());
        let mut native =
            SyntheticLabels::with_profile(SyntheticAllocationProfile::new(2628, 64).unwrap());
        assert_eq!(native.allocate().unwrap().magnitude(), 2628);
        assert_eq!(native.allocate().unwrap().magnitude(), 2692);
        assert_eq!(native.allocations(), 2);
        let mut edge =
            SyntheticLabels::with_profile(SyntheticAllocationProfile::new(u64::MAX, 1).unwrap());
        assert_eq!(edge.allocate().unwrap().magnitude(), u64::MAX);
        assert!(edge.allocate().is_err());
        assert_eq!(edge.allocations(), 1);
    }
    #[test]
    fn negative_identities_have_native_integer_order_and_no_zero() {
        assert!(SyntheticLabel::new(0).is_err());
        let minus_one = DataflowKey::from(SyntheticLabel::new(1).unwrap());
        let minus_two = DataflowKey::from(SyntheticLabel::new(2).unwrap());
        assert!(minus_two < minus_one && minus_one < DataflowKey::Core(CoreLabel(0)));
        assert_eq!(minus_two.native_integer(), "-2");
    }
}
