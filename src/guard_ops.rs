use crate::lattice::{BuiltinType, LatticeError, Type};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardEval {
    Positive,
    Negative,
    Unknown,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TypeTestResult {
    Impossible,
    Refined { argument: Type, result: Type },
}
pub fn eval_intersection(eval: GuardEval, actual: &Type) -> Result<Type, LatticeError> {
    let constraint = match eval {
        GuardEval::Positive => Type::atom("true"),
        GuardEval::Negative => Type::atom("false"),
        GuardEval::Unknown => actual.clone(),
    };
    constraint.meet(actual)
}
pub fn bind_type_test(
    eval: GuardEval,
    expected: &Type,
    actual: &Type,
) -> Result<TypeTestResult, LatticeError> {
    let (argument, result) = match eval {
        GuardEval::Positive => (expected.meet(actual)?, Type::atom("true")),
        GuardEval::Negative => (actual.subtract(expected)?, Type::atom("false")),
        GuardEval::Unknown => {
            return Ok(TypeTestResult::Refined {
                argument: actual.clone(),
                result: Type::builtin(BuiltinType::Boolean)?,
            });
        }
    };
    if argument.is_none() {
        Ok(TypeTestResult::Impossible)
    } else {
        Ok(TypeTestResult::Refined { argument, result })
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardTypeTest {
    Atom,
    Boolean,
    Binary,
    Bitstring,
    Float,
    Function,
    Integer,
    List,
    Map,
    Number,
    Pid,
    Port,
    Reference,
    Tuple,
}
impl GuardTypeTest {
    pub fn lookup(function: &str, arity: usize) -> Option<Self> {
        if arity != 1 {
            return None;
        }
        Some(match function {
            "is_atom" => Self::Atom,
            "is_boolean" => Self::Boolean,
            "is_binary" => Self::Binary,
            "is_bitstring" => Self::Bitstring,
            "is_float" => Self::Float,
            "is_function" => Self::Function,
            "is_integer" => Self::Integer,
            "is_list" => Self::List,
            "is_map" => Self::Map,
            "is_number" => Self::Number,
            "is_pid" => Self::Pid,
            "is_port" => Self::Port,
            "is_reference" => Self::Reference,
            "is_tuple" => Self::Tuple,
            _ => return None,
        })
    }
    pub fn domain(self) -> Result<Type, LatticeError> {
        Type::builtin(match self {
            Self::Atom => BuiltinType::Atom,
            Self::Boolean => BuiltinType::Boolean,
            Self::Binary => BuiltinType::Binary,
            Self::Bitstring => BuiltinType::Bitstring,
            Self::Float => BuiltinType::Float,
            Self::Function => BuiltinType::Function,
            Self::Integer => BuiltinType::Integer,
            Self::List => BuiltinType::MaybeImproperList,
            Self::Map => BuiltinType::Map,
            Self::Number => BuiltinType::Number,
            Self::Pid => BuiltinType::Pid,
            Self::Port => BuiltinType::Port,
            Self::Reference => BuiltinType::Reference,
            Self::Tuple => BuiltinType::Tuple,
        })
    }
}
pub fn type_test_domain(function: &str, arity: usize) -> Result<Type, LatticeError> {
    match GuardTypeTest::lookup(function, arity) {
        Some(test) => test.domain(),
        None => Ok(Type::any()),
    }
}
