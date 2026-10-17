use crate::bifs::registry::{Mfa, Registry, TransferError};
use crate::constraints::bindings::{BindingAccess, BindingKey, BindingMap};
use crate::contract_runtime::{ContractError, ContractSummary};
use crate::lattice::VariableId;
use crate::lattice::{Endpoint, Known, LatticeError, Type, TypeGetter, TypePredicate, TypeValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;
#[derive(Clone, Copy, Debug)]
pub enum BinaryMode {
    Construct,
    Match,
}
#[derive(Clone, Copy, Debug)]
pub struct SegmentUnit(u16);
impl SegmentUnit {
    pub fn get(self) -> u16 {
        self.0
    }
    pub fn new(value: u16) -> Result<Self, String> {
        if (1..=256).contains(&value) {
            Ok(Self(value))
        } else {
            Err("binary unit must be 1..256".into())
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum MapOperation {
    Associate,
    Exact,
}
#[derive(Clone, Debug)]
pub struct MapAssociation {
    pub key: Type,
    pub value: Type,
    pub operation: MapOperation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DerivedId(pub usize);
#[derive(Clone, Copy, Debug)]
pub enum Arithmetic {
    Add,
    Subtract,
    Multiply,
}
#[derive(Clone, Copy, Debug)]
pub enum KnownArgumentPosition {
    First,
    Second,
}
#[derive(Clone, Copy, Debug)]
pub enum OrderingOperation {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
}
#[derive(Clone, Debug)]
pub enum BooleanOperation {
    And,
    Or,
}
#[derive(Clone, Copy, Debug)]
pub enum EqualityKind {
    Exact,
    Coercing,
}
#[derive(Clone, Debug)]
pub enum DerivedOp {
    ContractReturn {
        summary: Arc<ContractSummary>,
        arguments: Vec<Type>,
        signature_return: Option<Type>,
    },
    RecordTestArgument {
        destination: Type,
        tag: Type,
        arity: Type,
        records: Arc<BTreeMap<(String, usize), Type>>,
    },
    FunctionArityArgument {
        destination: Type,
        arity: Type,
    },
    ClauseResidual {
        argument: Type,
        subtract: Vec<Type>,
    },
    BinarySegments {
        segments: Vec<Type>,
    },
    AppendHead {
        destination: Type,
    },
    AppendTail {
        destination: Type,
    },
    MapConstruct {
        argument: Type,
        entries: Vec<MapAssociation>,
    },
    MapValue {
        key: Type,
        map: Type,
        shadow_keys: Vec<Type>,
    },
    MapArgument {
        entries: Vec<MapAssociation>,
    },
    BinaryLength {
        size: Type,
        unit: SegmentUnit,
        mode: BinaryMode,
    },
    BinaryValue {
        size: Type,
        unit: SegmentUnit,
        signed: bool,
    },
    Cons {
        head: Type,
        tail: Type,
    },
    ConsHead {
        value: Type,
    },
    ConsTail {
        value: Type,
    },
    BinaryConcat {
        left: Type,
        right: Type,
    },
    BinaryMatch {
        left: Type,
        right: Type,
    },
    BooleanArgument {
        operation: BooleanOperation,
        destination: Type,
        other: Type,
    },
    BooleanReturn {
        operation: BooleanOperation,
        left: Type,
        right: Type,
    },
    BooleanNot {
        value: Type,
    },
    EqualityArgument {
        kind: EqualityKind,
        destination: Type,
        argument: Type,
        other: Type,
    },
    ExactEqualityReturn {
        left: Type,
        right: Type,
    },
    BifReturn {
        mfa: Mfa,
        arguments: Vec<Type>,
    },
    InverseArithmetic {
        operation: Arithmetic,
        known_position: KnownArgumentPosition,
        destination: Type,
        other: Type,
    },
    OrderingArgument {
        operation: OrderingOperation,
        destination: Type,
        argument: Type,
        other: Type,
    },
    PredicateArgument {
        destination: Type,
        test: Type,
    },
    PredicateReturn {
        argument: Type,
        test: Type,
    },
}
#[derive(Clone, Debug)]
pub enum EvalError {
    Native(LatticeError),
    Transfer(TransferError),
    Contract(ContractError),
}
impl From<LatticeError> for EvalError {
    fn from(e: LatticeError) -> Self {
        Self::Native(e)
    }
}
impl From<ContractError> for EvalError {
    fn from(e: ContractError) -> Self {
        match e {
            ContractError::Lattice(e) => Self::Native(e),
            e => Self::Contract(e),
        }
    }
}
impl From<TransferError> for EvalError {
    fn from(e: TransferError) -> Self {
        Self::Transfer(e)
    }
}
pub trait BifResultProvider {
    /// Opt-in contract for root replay: result and error behavior
    /// are deterministic for fixed arguments and configuration throughout a
    /// borrowed solve, and calls have no externally observable effects. This
    /// grants no thread-safety capability. Arbitrary providers remain opaque.
    fn replay_is_pure(&self) -> bool {
        false
    }
    fn result(&self, mfa: &Mfa, arguments: &[Type]) -> Result<Type, EvalError>;
}
impl BifResultProvider for Registry {
    fn replay_is_pure(&self) -> bool {
        true
    }
    fn result(&self, mfa: &Mfa, arguments: &[Type]) -> Result<Type, EvalError> {
        Ok(self.transfer(mfa, arguments)?.result)
    }
}
#[derive(Clone, Debug, Default)]
pub struct DerivedArena {
    nodes: Vec<DerivedOp>,
    dependencies: Vec<BTreeSet<VariableId>>,
    #[cfg(otp_domain = "29")]
    pub(crate) record_layout: Option<Arc<crate::native_runtime_hash::NativeAtomLayout>>,
}
impl DerivedArena {
    /// Appends `other`'s nodes after this arena's; returns the id offset.
    pub fn append(&mut self, other: DerivedArena) -> usize {
        let offset = self.nodes.len();
        self.nodes.extend(other.nodes);
        self.dependencies.extend(other.dependencies);
        offset
    }
    pub fn push(&mut self, op: DerivedOp) -> DerivedId {
        let inputs = op.inputs();
        let mut dependencies = BTreeSet::new();
        // The original product walk pops its inputs in reverse order.
        for input in inputs.iter().rev() {
            input.append_var_names(&mut dependencies);
        }
        if crate::lattice::verify_fast_paths() {
            let product = Type::product(inputs.into_iter().copied().collect());
            let mut expected = BTreeSet::new();
            product.append_var_names_reference(&mut expected);
            assert_eq!(dependencies, expected, "derived input variable union");
        }
        let id = DerivedId(self.nodes.len());
        self.nodes.push(op);
        self.dependencies.push(dependencies);
        id
    }
    pub fn dependencies(&self, id: DerivedId) -> Option<&BTreeSet<VariableId>> {
        self.dependencies.get(id.0)
    }
    pub fn get(&self, id: DerivedId) -> Option<&DerivedOp> {
        self.nodes.get(id.0)
    }
}
impl DerivedOp {
    fn inputs(&self) -> Vec<&Type> {
        match self {
            Self::ContractReturn { arguments, .. } => arguments.iter().collect(),
            Self::RecordTestArgument {
                destination,
                tag,
                arity,
                ..
            } => vec![tag, arity, destination],
            Self::FunctionArityArgument { destination, arity } => vec![destination, arity],
            Self::ClauseResidual { argument, .. } => vec![argument],
            Self::BinarySegments { segments } => segments.iter().collect(),
            Self::AppendHead { destination } | Self::AppendTail { destination } => {
                vec![destination]
            }
            Self::MapConstruct { argument, entries } => std::iter::once(argument)
                .chain(entries.iter().flat_map(|p| [&p.key, &p.value]))
                .collect(),
            Self::MapValue {
                key,
                map,
                shadow_keys,
            } => [key, map].into_iter().chain(shadow_keys).collect(),
            Self::MapArgument { entries } => entries.iter().map(|p| &p.key).collect(),
            Self::BinaryLength { size, .. } | Self::BinaryValue { size, .. } => vec![size],
            Self::Cons { head, tail } => vec![head, tail],
            Self::ConsHead { value } | Self::ConsTail { value } => vec![value],
            Self::BinaryConcat { left, right } | Self::BinaryMatch { left, right } => {
                vec![left, right]
            }
            Self::BooleanArgument {
                destination, other, ..
            } => vec![other, destination],
            Self::BooleanReturn { left, right, .. } | Self::ExactEqualityReturn { left, right } => {
                vec![left, right]
            }
            Self::BooleanNot { value } => vec![value],
            Self::EqualityArgument {
                destination,
                argument,
                other,
                ..
            } => vec![destination, argument, other],
            Self::BifReturn { arguments, .. } => arguments.iter().collect(),
            Self::InverseArithmetic {
                destination, other, ..
            } => vec![destination, other],
            Self::OrderingArgument {
                destination,
                argument,
                other,
                ..
            } => vec![destination, argument, other],
            Self::PredicateArgument { destination, .. } => vec![destination],
            Self::PredicateReturn { argument, .. } => vec![argument],
        }
    }
    pub fn evaluate(
        &self,
        map: &BindingMap,
        provider: &dyn BifResultProvider,
    ) -> Result<Type, EvalError> {
        self.evaluate_access(map, provider)
    }
    pub(super) fn evaluate_access<A: BindingAccess>(
        &self,
        map: &A,
        provider: &dyn BifResultProvider,
    ) -> Result<Type, EvalError> {
        let bindings = |id: &VariableId| map.get(&BindingKey::Label(id.clone()));
        let lookup = |ty: &Type| ty.substitute_with(&bindings);
        let call = |name: &str, args: &[Type]| {
            provider.result(
                &Mfa {
                    module: "erlang".into(),
                    function: name.into(),
                    arity: args.len(),
                },
                args,
            )
        };
        Ok(match self {
            Self::ContractReturn {
                summary,
                arguments,
                signature_return,
            } => {
                let arguments = arguments
                    .iter()
                    .map(lookup)
                    .collect::<Result<Vec<_>, _>>()?;
                let result = summary.return_for(&arguments)?;
                if let Some(sig) = signature_return {
                    result.meet(sig)?
                } else {
                    result
                }
            }
            Self::ClauseResidual { argument, subtract } => {
                lookup(argument)?.subtract_types(subtract)?
            }
            Self::RecordTestArgument {
                destination,
                tag,
                arity,
                records,
            } => {
                if !lookup(destination)?.is_atom_literal("true")? {
                    Type::any()
                } else {
                    let arity = lookup(arity)?;
                    if !arity.predicate(TypePredicate::Integer)? {
                        let tuple = Type::tuple_any();
                        #[cfg(otp_domain = "29")]
                        let tuple = tuple.join(&Type::record_any())?;
                        tuple
                    } else {
                        let TypeValue::Integers(values) = arity.get(TypeGetter::NumberValues)?
                        else {
                            unreachable!("typed integer values")
                        };
                        match values {
                            Known::Known(values) if values.len() == 1 => {
                                let value = &values[0];
                                let tag = lookup(tag)?;
                                if tag.predicate(TypePredicate::Atom)? {
                                    if value <= &BigInt::from(0) {
                                        return Err(LatticeError::FunctionClause.into());
                                    }
                                    let arity = value.to_usize().filter(|value| *value <= 1_000_000)
                                        .ok_or_else(|| EvalError::Transfer(TransferError::Unsupported {
                                            mfa: Mfa { module: "erlang".into(), function: "is_record".into(), arity: 3 }, family: "record arity materialization exceeds explicit budget".into(),
                                        }))?;
                                    let mut elements = vec![Type::any(); arity];
                                    elements[0] = tag.clone();
                                    let generic = Type::tuple(elements)?;
                                    let TypeValue::Atoms(tags) = tag.get(TypeGetter::AtomValues)?
                                    else {
                                        unreachable!("typed atom values")
                                    };
                                    match tags {
                                        Known::Known(tags) if tags.len() == 1 => records
                                            .get(&(tags[0].clone(), arity - 1))
                                            .cloned()
                                            .unwrap_or(generic),
                                        _ => generic,
                                    }
                                } else if value > &BigInt::from(1024) {
                                    Type::tuple_any()
                                } else {
                                    Type::tuple_arity(
                                        value.to_usize().ok_or(LatticeError::FunctionClause)?,
                                    )
                                }
                            }
                            _ => Type::tuple_any(),
                        }
                    }
                }
            }
            Self::FunctionArityArgument { destination, arity } => {
                if lookup(destination)?.is_atom_literal("true")? {
                    match lookup(arity)?.get(TypeGetter::NumberValues)? {
                        TypeValue::Integers(Known::Unknown) => Type::function(None, Type::any()),
                        TypeValue::Integers(Known::Known(values)) => {
                            let mut result = Type::none();
                            for arity in values {
                                let arity = arity.to_i64().ok_or(LatticeError::FunctionClause)?;
                                result =
                                    result.join(&Type::function_with_arity(arity, Type::any())?)?;
                            }
                            result
                        }
                        _ => unreachable!("number-values accessor has typed integer-values output"),
                    }
                } else {
                    Type::any()
                }
            }
            Self::BinarySegments { segments } => Type::bitstr_concat_types(
                &segments.iter().map(lookup).collect::<Result<Vec<_>, _>>()?,
            )?,
            Self::MapConstruct { argument, entries } => {
                let mut result = Type::map_any().meet(&lookup(argument)?)?;
                for entry in entries {
                    result = result.map_put(lookup(&entry.key)?, lookup(&entry.value)?)?;
                }
                result
            }
            Self::MapValue {
                key,
                map,
                shadow_keys,
            } => {
                let key = lookup(key)?;
                if !key.predicate(TypePredicate::Singleton)? {
                    return Ok(Type::any());
                }
                let map = lookup(map)?.meet(&Type::map_any())?;
                if map.predicate(TypePredicate::Impossible)? {
                    return Ok(Type::none());
                }
                for shadow in shadow_keys {
                    if !lookup(shadow)?
                        .meet(&key)?
                        .predicate(TypePredicate::Impossible)?
                    {
                        return Ok(Type::any());
                    }
                }
                map.map_get(&key)?
            }
            Self::MapArgument { entries } => {
                let mut result = Type::map_any();
                let mut shadow = Type::none();
                for entry in entries {
                    let key = lookup(&entry.key)?;
                    if matches!(entry.operation, MapOperation::Exact)
                        && shadow.meet(&key)?.predicate(TypePredicate::Impossible)?
                    {
                        result = result.map_put(key.clone(), Type::any())?;
                    }
                    shadow = key.join(&shadow)?;
                }
                result
            }
            Self::AppendHead { destination } | Self::AppendTail { destination } => {
                let dst = lookup(destination)?;
                let head = matches!(self, Self::AppendHead { .. });
                if dst.predicate(TypePredicate::Cons)? {
                    if head {
                        Type::proper_list(get_type(&dst, TypeGetter::ConsHead)?, false)
                    } else {
                        get_type(&dst, TypeGetter::ConsTail)?.join(&dst)?
                    }
                } else if dst.predicate(TypePredicate::List)? {
                    if dst.predicate(TypePredicate::Nil)? {
                        dst
                    } else {
                        Type::proper_list(get_type(&dst, TypeGetter::ListElements)?, false)
                    }
                } else if head {
                    Type::proper_list(Type::any(), false)
                } else {
                    Type::any()
                }
            }
            Self::BinaryLength { size, unit, mode } => {
                let size = lookup(size)?;
                let unit = BigInt::from(unit.0);
                let (period, base) = if nonnegative_integer(&size)? {
                    let TypeValue::Integers(values) = size.get(TypeGetter::NumberValues)? else {
                        unreachable!("integer values")
                    };
                    if let Known::Known(values) = values
                        && values.len() == 1
                    {
                        (
                            BigInt::from(matches!(mode, BinaryMode::Match) as u8),
                            &values[0] * &unit,
                        )
                    } else {
                        let TypeValue::Endpoint(Endpoint::Finite(min)) =
                            size.get(TypeGetter::NumberMin)?
                        else {
                            unreachable!("nonnegative minimum")
                        };
                        (unit.clone(), min * &unit)
                    }
                } else {
                    (unit, 0.into())
                };
                Type::bitstring(period, base).map_err(|_| LatticeError::BadArg)?
            }
            Self::BinaryValue { size, unit, signed } => {
                let size = lookup(size)?;
                if nonnegative_integer(&size)? {
                    let TypeValue::Endpoint(max) = size.get(TypeGetter::NumberMax)? else {
                        unreachable!("number maximum")
                    };
                    if let Endpoint::Finite(max) = max
                        && let Some(max) = max.to_u32()
                        && max < 128
                    {
                        let bits = max * u32::from(unit.0);
                        let (min, max) = if *signed {
                            let n = if bits == 0 {
                                BigInt::from(0)
                            } else {
                                BigInt::from(1) << ((bits - 1) as usize)
                            };
                            (-&n, n - 1)
                        } else {
                            (0.into(), (BigInt::from(1) << (bits as usize)) - 1)
                        };
                        return Ok(Type::range(Endpoint::Finite(min), Endpoint::Finite(max))
                            .expect("native binary integer bounds"));
                    }
                }
                Type::all_integers()
            }
            Self::Cons { head, tail } => Type::cons(lookup(head)?, lookup(tail)?)?,
            Self::ConsHead { value } | Self::ConsTail { value } => {
                let value = lookup(value)?;
                if !value.predicate(TypePredicate::Cons)? {
                    return Ok(Type::any());
                }
                let getter = if matches!(self, Self::ConsHead { .. }) {
                    TypeGetter::ConsHead
                } else {
                    TypeGetter::ConsTail
                };
                let TypeValue::Type(ty) = value.get(getter)? else {
                    unreachable!("typed cons getter")
                };
                ty
            }
            Self::BinaryConcat { left, right } => lookup(left)?.bitstr_concat(&lookup(right)?)?,
            Self::BinaryMatch { left, right } => lookup(left)?.bitstr_match(&lookup(right)?)?,
            Self::BooleanArgument {
                operation,
                destination,
                other,
            } => {
                let dst = lookup(destination)?;
                match operation {
                    BooleanOperation::And => {
                        if dst.is_atom_literal("true")? {
                            Type::atom("true")
                        } else if dst.is_atom_literal("false")?
                            && lookup(other)?.is_atom_literal("true")?
                        {
                            Type::atom("false")
                        } else {
                            boolean()
                        }
                    }
                    BooleanOperation::Or => {
                        if dst.is_atom_literal("false")? {
                            Type::atom("false")
                        } else if dst.is_atom_literal("true")?
                            && lookup(other)?.is_atom_literal("false")?
                        {
                            Type::atom("true")
                        } else {
                            boolean()
                        }
                    }
                }
            }
            Self::BooleanReturn {
                operation,
                left,
                right,
            } => {
                let left = lookup(left)?;
                match operation {
                    BooleanOperation::And => {
                        if left.is_atom_literal("false")? {
                            Type::atom("false")
                        } else {
                            let right = lookup(right)?;
                            if right.is_atom_literal("false")? {
                                Type::atom("false")
                            } else if left.is_atom_literal("true")?
                                && right.is_atom_literal("true")?
                            {
                                Type::atom("true")
                            } else {
                                boolean()
                            }
                        }
                    }
                    BooleanOperation::Or => {
                        if left.is_atom_literal("true")? {
                            Type::atom("true")
                        } else {
                            let right = lookup(right)?;
                            if right.is_atom_literal("true")? {
                                Type::atom("true")
                            } else if left.is_atom_literal("false")?
                                && right.is_atom_literal("false")?
                            {
                                Type::atom("false")
                            } else {
                                boolean()
                            }
                        }
                    }
                }
            }
            Self::BooleanNot { value } => {
                let value = lookup(value)?;
                if value.is_atom_literal("true")? {
                    Type::atom("false")
                } else if value.is_atom_literal("false")? {
                    Type::atom("true")
                } else {
                    boolean()
                }
            }
            Self::ExactEqualityReturn { left, right } => {
                if lookup(left)?
                    .meet(&lookup(right)?)?
                    .predicate(TypePredicate::None)?
                {
                    Type::atom("false")
                } else {
                    boolean()
                }
            }
            Self::EqualityArgument {
                kind,
                destination,
                argument,
                other,
            } => {
                let (dst, other) = match kind {
                    EqualityKind::Exact => (lookup(destination)?, lookup(other)?),
                    EqualityKind::Coercing => {
                        let other = lookup(other)?;
                        (lookup(destination)?, other)
                    }
                };
                match kind {
                    EqualityKind::Exact => {
                        if dst.is_atom_literal("true")? {
                            other
                        } else if dst.is_atom_literal("false")? && solver_singleton(&other)? {
                            lookup(argument)?.subtract(&other)?
                        } else {
                            Type::any()
                        }
                    }
                    EqualityKind::Coercing => {
                        if !other.predicate(TypePredicate::Number)? && solver_singleton(&other)? {
                            if dst.is_atom_literal("true")? {
                                other
                            } else if dst.is_atom_literal("false")? {
                                lookup(argument)?.subtract(&other)?
                            } else {
                                Type::any()
                            }
                        } else if dst.is_atom_literal("true")? {
                            if other.predicate(TypePredicate::Number)? {
                                Type::number()
                            } else if other.predicate(TypePredicate::Atom)? {
                                other
                            } else {
                                Type::any()
                            }
                        } else {
                            Type::any()
                        }
                    }
                }
            }
            Self::BifReturn { mfa, arguments } => provider.result(
                mfa,
                &arguments
                    .iter()
                    .map(lookup)
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
            Self::InverseArithmetic {
                operation,
                known_position,
                destination,
                other,
            } => {
                let dst = lookup(destination)?;
                let other = lookup(other)?;
                if dst.predicate(TypePredicate::Integer)? {
                    if other.predicate(TypePredicate::Integer)? {
                        match operation {
                            Arithmetic::Add => call("-", &[dst, other])?,
                            Arithmetic::Subtract => match known_position {
                                KnownArgumentPosition::First => call("-", &[other, dst])?,
                                KnownArgumentPosition::Second => call("+", &[other, dst])?,
                            },
                            Arithmetic::Multiply => {
                                let zero = Type::integer(0.into());
                                if !other.meet(&zero)?.predicate(TypePredicate::None)? {
                                    Type::all_integers()
                                } else {
                                    let ret = call("div", &[dst.clone(), other])?;
                                    if !zero.is_subtype(&dst)? {
                                        ret.subtract(&zero)?
                                    } else {
                                        ret
                                    }
                                }
                            }
                        }
                    } else {
                        Type::all_integers()
                    }
                } else if dst.predicate(TypePredicate::Float)? {
                    if other.predicate(TypePredicate::Integer)? {
                        Type::float()
                    } else {
                        Type::number()
                    }
                } else {
                    Type::number()
                }
            }
            Self::OrderingArgument {
                operation,
                destination,
                argument,
                other,
            } => {
                let dst = lookup(destination)?;
                let yes = dst.is_atom_literal("true")?;
                let no = dst.is_atom_literal("false")?;
                if yes || no {
                    let argument = lookup(argument)?;
                    let other = lookup(other)?;
                    if argument.predicate(TypePredicate::Integer)?
                        && other.predicate(TypePredicate::Integer)?
                    {
                        let min1 = endpoint(&argument, TypeGetter::NumberMin)?;
                        let max1 = endpoint(&argument, TypeGetter::NumberMax)?;
                        let min2 = endpoint(&other, TypeGetter::NumberMin)?;
                        let max2 = endpoint(&other, TypeGetter::NumberMax)?;
                        let (min, max) = match (operation, yes) {
                            (OrderingOperation::LessEqual, true) => (min1, max2),
                            (OrderingOperation::LessEqual, false) => (increment(min2), max1),
                            (OrderingOperation::Less, true) => (min1, decrement(max2)),
                            (OrderingOperation::Less, false) => (min2, max1),
                            (OrderingOperation::GreaterEqual, true) => (min2, max1),
                            (OrderingOperation::GreaterEqual, false) => (min1, decrement(max2)),
                            (OrderingOperation::Greater, true) => (increment(min2), max1),
                            (OrderingOperation::Greater, false) => (min1, max2),
                        };
                        Type::range(min, max).expect("typed integer endpoint range")
                    } else {
                        Type::any()
                    }
                } else {
                    Type::any()
                }
            }
            Self::PredicateArgument { destination, test } => {
                if lookup(destination)?.is_atom_literal("true")? {
                    test.clone()
                } else {
                    Type::any()
                }
            }
            Self::PredicateReturn { argument, test } => {
                let argument = lookup(argument)?;
                if argument.meet(test)?.predicate(TypePredicate::None)? {
                    Type::atom("false")
                } else if argument.is_subtype(test)? {
                    Type::atom("true")
                } else {
                    Type::atoms(["false", "true"])
                }
            }
        })
    }
}
fn endpoint(ty: &Type, getter: TypeGetter) -> Result<Endpoint, LatticeError> {
    let TypeValue::Endpoint(v) = ty.get(getter)? else {
        unreachable!("typed numeric endpoint getter")
    };
    Ok(v)
}
fn increment(v: Endpoint) -> Endpoint {
    match v {
        Endpoint::Finite(n) => Endpoint::Finite(n + 1),
        v => v,
    }
}
fn decrement(v: Endpoint) -> Endpoint {
    match v {
        Endpoint::Finite(n) => Endpoint::Finite(n - 1),
        v => v,
    }
}

fn boolean() -> Type {
    Type::atoms(["false", "true"])
}
fn solver_singleton(ty: &Type) -> Result<bool, LatticeError> {
    if ty.predicate(TypePredicate::Atom)? {
        let TypeValue::Atoms(values) = ty.get(TypeGetter::AtomValues)? else {
            unreachable!("typed atom values getter")
        };
        Ok(matches!(values,Known::Known(v)if v.len()==1))
    } else if ty.predicate(TypePredicate::Integer)? {
        let TypeValue::Integers(values) = ty.get(TypeGetter::NumberValues)? else {
            unreachable!("typed integer values getter")
        };
        Ok(matches!(values,Known::Known(v)if v.len()==1))
    } else {
        ty.predicate(TypePredicate::Nil)
    }
}

pub struct NoBifProvider;
impl BifResultProvider for NoBifProvider {
    fn result(&self, mfa: &Mfa, _arguments: &[Type]) -> Result<Type, EvalError> {
        Err(EvalError::Transfer(TransferError::Unsupported {
            mfa: mfa.clone(),
            family: "derived BIF requires a typed provider".into(),
        }))
    }
}

fn nonnegative_integer(value: &Type) -> Result<bool, LatticeError> {
    Ok(value.predicate(TypePredicate::Integer)?
        && value.is_subtype(
            &Type::range(Endpoint::Finite(0.into()), Endpoint::PositiveInfinity)
                .expect("nonnegative range"),
        )?)
}

fn get_type(value: &Type, getter: TypeGetter) -> Result<Type, LatticeError> {
    let TypeValue::Type(ty) = value.get(getter)? else {
        unreachable!("typed structural getter")
    };
    Ok(ty)
}

#[cfg(test)]
mod borrowed_substitution_tests {
    use super::*;
    use crate::lattice::{MapTypeEntry, Presence};
    use std::cell::RefCell;

    struct Capture(RefCell<Vec<Type>>);
    impl BifResultProvider for Capture {
        fn result(&self, _: &Mfa, arguments: &[Type]) -> Result<Type, EvalError> {
            *self.0.borrow_mut() = arguments.to_vec();
            Ok(arguments[0].clone())
        }
    }

    #[test]
    fn provider_receives_one_step_label_substitutions_in_argument_order() {
        let one = VariableId::Integer(1.into());
        let two = VariableId::Integer(2.into());
        let mut bindings = BindingMap::new();
        bindings.insert(
            BindingKey::Label(one.clone()),
            Type::variable_integer(2.into()),
        );
        bindings.insert(BindingKey::Label(two), Type::atom("target"));
        bindings.insert(BindingKey::RawVariableRecord(one), Type::atom("raw"));
        bindings.insert(
            BindingKey::RawVariableRecord(VariableId::Integer(3.into())),
            Type::atom("raw_only"),
        );
        let capture = Capture(RefCell::new(vec![]));
        let op = DerivedOp::BifReturn {
            mfa: Mfa {
                module: "proof".into(),
                function: "capture".into(),
                arity: 3,
            },
            arguments: vec![
                Type::variable_integer(1.into()),
                Type::variable_integer(2.into()),
                Type::variable_integer(3.into()),
            ],
        };
        assert_eq!(
            op.evaluate(&bindings, &capture).unwrap(),
            Type::variable_integer(2.into())
        );
        assert_eq!(
            *capture.0.borrow(),
            vec![
                Type::variable_integer(2.into()),
                Type::atom("target"),
                Type::any()
            ]
        );
    }

    #[test]
    fn provider_receives_map_entry_binding_absent_from_variable_inventory() {
        let value = Type::map_types(
            vec![MapTypeEntry {
                key: Type::atom("entry"),
                presence: Presence::Mandatory,
                value: Type::variable_integer(1.into()),
            }],
            Type::all_atoms(),
            Type::variable_integer(2.into()),
        )
        .unwrap();
        let mut names = std::collections::BTreeSet::new();
        value.append_var_names(&mut names);
        assert!(
            !names.contains(&VariableId::Integer(1.into()))
        );
        let bindings = BindingMap::from(vec![
            (
                BindingKey::Label(VariableId::Integer(1.into())),
                Type::atom("entry_value"),
            ),
            (
                BindingKey::Label(VariableId::Integer(2.into())),
                Type::atom("default_value"),
            ),
        ]);
        let expected = value
            .substitute(&BTreeMap::from([
                (VariableId::Integer(1.into()), Type::atom("entry_value")),
                (VariableId::Integer(2.into()), Type::atom("default_value")),
            ]))
            .unwrap();
        let capture = Capture(RefCell::new(vec![]));
        let op = DerivedOp::BifReturn {
            mfa: Mfa {
                module: "proof".into(),
                function: "capture".into(),
                arity: 1,
            },
            arguments: vec![value],
        };
        assert_eq!(op.evaluate(&bindings, &capture).unwrap(), expected);
        assert_eq!(*capture.0.borrow(), vec![expected]);
    }
}

#[cfg(test)]
mod direct_input_inventory_tests {
    use super::*;
    #[test]
    fn derived_input_union_matches_empty_singleton_and_raw_products() {
        let x = Type::variable_atom("x");
        for inputs in [vec![], vec![Type::none()], vec![x], vec![Type::none(), x],
            vec![Type::unit(), x], vec![x, x, Type::none(), Type::unit()]] {
            let expected = if inputs.contains(&x) { BTreeSet::from([VariableId::Atom("x".into())]) }
                else { BTreeSet::new() };
            let mut arena = DerivedArena::default();
            let id = arena.push(DerivedOp::BinarySegments { segments: inputs });
            assert_eq!(arena.dependencies(id).unwrap(), &expected);
        }
    }
}
