use crate::lattice::{LatticeError, Type, TypeGetter, TypePredicate, TypeValue};
use crate::returns::ReturnKind;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SourceAnnotation {
    pub line: Option<i64>,
    pub compiler_generated: bool,
}
impl SourceAnnotation {
    pub fn is_generated(self) -> bool {
        self.compiler_generated || self.line.is_none_or(|line| line < 1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WarningState<W> {
    pub warning_mode: bool,
    pub warnings: Vec<W>,
}
impl<W: Clone + PartialEq> WarningState<W> {
    pub fn add(&mut self, annotation: SourceAnnotation, warning: W, forced: bool) {
        if self.warning_mode && (forced || !annotation.is_generated()) {
            self.warnings.insert(0, warning);
        }
    }
    pub fn add_all(&mut self, mut warnings: Vec<W>) {
        warnings.append(&mut self.warnings);
        self.warnings = warnings;
    }
    pub fn remove_added(&mut self, old: &[W]) -> Vec<W> {
        if self.warnings == old {
            return Vec::new();
        }
        let mut added = self.warnings.clone();
        for previous in old {
            if let Some(index) = added.iter().position(|warning| warning == previous) {
                added.remove(index);
            }
        }
        self.warnings = old.to_vec();
        added
    }
}

pub struct ParentEvidence<'a> {
    pub escaping: bool,
    pub incoming_count: usize,
    pub parent_named: bool,
    pub validated_parent_contract_return: Option<&'a Type>,
}
pub struct FunctionEvidence<'a> {
    pub named: bool,
    pub handled: bool,
    pub reachable: bool,
    pub result: &'a Type,
    pub validated_own_contract_return: Option<&'a Type>,
    pub parent: ParentEvidence<'a>,
    pub return_kind: ReturnKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FunctionWarning {
    Unused,
    NoReturn { kind: ReturnKind },
    OnlyExit,
}
fn parent_allows_this(named: bool, parent: &ParentEvidence<'_>) -> Result<bool, LatticeError> {
    if !parent.escaping || named || parent.incoming_count != 1 || !parent.parent_named {
        return Ok(false);
    }
    let Some(result) = parent.validated_parent_contract_return else {
        return Ok(false);
    };
    if !result.predicate(TypePredicate::Function)? {
        return Ok(false);
    }
    let TypeValue::Type(range) = result.get(TypeGetter::FunctionRange)? else {
        unreachable!("canonical function range getter returns a type")
    };
    Ok(range.is_unit())
}
pub fn function_warning(
    evidence: &FunctionEvidence<'_>,
) -> Result<Option<FunctionWarning>, LatticeError> {
    if !evidence.handled {
        return Ok(evidence.named.then_some(FunctionWarning::Unused));
    }
    if !evidence.result.is_none() {
        return Ok(None);
    }
    let allowed = match evidence.validated_own_contract_return {
        Some(result) => result.is_unit(),
        None => parent_allows_this(evidence.named, &evidence.parent)?,
    };
    if allowed || !(evidence.named || evidence.reachable) {
        return Ok(None);
    }
    Ok(Some(match evidence.return_kind {
        ReturnKind::OnlyExplicit => FunctionWarning::OnlyExit,
        kind => FunctionWarning::NoReturn { kind },
    }))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum EqualityOperator {
    ExactEqual,
    ExactNotEqual,
    Equal,
    NotEqual,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub enum ComparisonWarning {
    Opaque {
        left: Type,
        operator: EqualityOperator,
        right: Type,
    },
    Exact {
        left: Type,
        operator: EqualityOperator,
        right: Type,
    },
}
pub fn comparison_warnings(
    left: &Type,
    operator: EqualityOperator,
    right: &Type,
    module: &str,
) -> Result<Vec<ComparisonWarning>, LatticeError> {
    use crate::lattice::OpacityConflict;
    let mut warnings = Vec::new();
    let conflict = left.opacity_conflict(right, module)?;
    if !(left.is_none() || right.is_none()) {
        match conflict {
            OpacityConflict::ExpectedTransparent => warnings.push(ComparisonWarning::Opaque {
                left: right.clone(),
                operator,
                right: left.clone(),
            }),
            OpacityConflict::ExpectedOpaque => warnings.push(ComparisonWarning::Opaque {
                left: left.clone(),
                operator,
                right: right.clone(),
            }),
            OpacityConflict::None => {}
        }
    }
    let disjoint = left.meet(right)?.is_none();
    let inexact_integer_float = matches!(
        operator,
        EqualityOperator::Equal | EqualityOperator::NotEqual
    ) && ((left.predicate(TypePredicate::Float)?
        && right.predicate(TypePredicate::Integer)?)
        || (left.predicate(TypePredicate::Integer)? && right.predicate(TypePredicate::Float)?));
    if disjoint && !inexact_integer_float {
        warnings.insert(
            0,
            ComparisonWarning::Exact {
                left: left.clone(),
                operator,
                right: right.clone(),
            },
        );
    }
    Ok(warnings)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyOverride {
    Inherit,
    Enabled,
    Disabled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PolicyFacts {
    pub named_owner: bool,
    pub module_metadata_present: bool,
    pub module_tag_enabled: bool,
    pub module_function_suppressed: bool,
    pub function_tag: PropertyOverride,
    pub function_suppression: PropertyOverride,
}
impl PolicyFacts {
    pub fn allows(self) -> bool {
        let resolve = |value, fallback| match value {
            PropertyOverride::Inherit => fallback,
            PropertyOverride::Enabled => true,
            PropertyOverride::Disabled => false,
        };
        let tag = resolve(
            if self.named_owner {
                self.function_tag
            } else {
                PropertyOverride::Inherit
            },
            !self.module_metadata_present || self.module_tag_enabled,
        );
        let suppressed = self.named_owner
            && resolve(
                self.function_suppression,
                self.module_metadata_present && self.module_function_suppressed,
            );
        tag && !suppressed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removing_added_warnings_preserves_native_duplicate_subtraction() {
        let mut state = WarningState {
            warning_mode: false,
            warnings: vec!["a", "b", "a"],
        };
        state.add_all(vec!["a", "a", "b"]);
        assert_eq!(state.remove_added(&["a", "b", "a"]), vec!["a", "b", "a"]);
        assert_eq!(state.warnings, vec!["a", "b", "a"]);
    }
    #[test]
    fn force_cannot_enable_warnings_during_inference() {
        let mut state = WarningState {
            warning_mode: false,
            warnings: Vec::new(),
        };
        state.add(
            SourceAnnotation {
                line: Some(1),
                compiler_generated: false,
            },
            "warning",
            true,
        );
        assert!(state.warnings.is_empty());
    }
    #[test]
    fn unit_return_is_not_no_return() {
        let result = Type::unit();
        let evidence = FunctionEvidence {
            named: true,
            handled: true,
            reachable: true,
            result: &result,
            validated_own_contract_return: None,
            parent: ParentEvidence {
                escaping: false,
                incoming_count: 0,
                parent_named: false,
                validated_parent_contract_return: None,
            },
            return_kind: ReturnKind::OnlyNormal,
        };
        assert_eq!(function_warning(&evidence).unwrap(), None);
    }
}
