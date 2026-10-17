use super::*;

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RuntimeTerm(Term);
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Term {
    Atom(String),
    Integer(BigInt),
    Float(u64),
    Bitstring(BigInt, usize),
    Function(String, usize),
    Pid(String),
    Port(String),
    Reference(String),
    Nil,
    Cons(Box<RuntimeTerm>, Box<RuntimeTerm>),
    Tuple(Vec<RuntimeTerm>),
    Map(Vec<(RuntimeTerm, RuntimeTerm)>),
    #[cfg(otp_domain = "29")]
    Record { module: String, name: String, fields: BTreeMap<String, RuntimeTerm> },
}
impl RuntimeTerm {
    pub fn atom(value: impl Into<String>) -> Self {
        Self(Term::Atom(value.into()))
    }
    pub fn integer(value: BigInt) -> Self {
        Self(Term::Integer(value))
    }
    pub fn float(value: f64) -> Result<Self, String> {
        if !value.is_finite() {
            return Err("Erlang runtime floats must be finite".into());
        }
        Ok(Self(Term::Float(value.to_bits())))
    }
    pub fn bitstring(value: BigInt, bits: usize) -> Result<Self, String> {
        if value < BigInt::zero() || value.bits() > bits as u64 {
            return Err("bitstring value exceeds declared unsigned bit size".into());
        }
        Ok(Self(Term::Bitstring(value, bits)))
    }
    pub fn function(identity: impl Into<String>, arity: usize) -> Result<Self, String> {
        if arity > 255 {
            return Err("Erlang function arity exceeds 255".into());
        }
        Ok(Self(Term::Function(identity.into(), arity)))
    }
    pub fn pid(identity: impl Into<String>) -> Self {
        Self(Term::Pid(identity.into()))
    }
    pub fn port(identity: impl Into<String>) -> Self {
        Self(Term::Port(identity.into()))
    }
    pub fn reference(identity: impl Into<String>) -> Self {
        Self(Term::Reference(identity.into()))
    }
    pub fn nil() -> Self {
        Self(Term::Nil)
    }
    pub fn cons(head: Self, tail: Self) -> Self {
        Self(Term::Cons(Box::new(head), Box::new(tail)))
    }
    pub fn tuple(elements: Vec<Self>) -> Self {
        Self(Term::Tuple(elements))
    }
    pub fn map(pairs: Vec<(Self, Self)>) -> Self {
        let mut keys = BTreeMap::new();
        for (key, value) in pairs {
            keys.insert(key, value);
        }
        Self(Term::Map(keys.into_iter().collect()))
    }
    #[cfg(otp_domain = "29")]
    pub fn record(module: impl Into<String>, name: impl Into<String>, fields: BTreeMap<String, Self>) -> Self {
        Self(Term::Record { module: module.into(), name: name.into(), fields })
    }
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let kind = value["term"].as_str().ok_or("missing runtime term kind")?;
        let identity = || {
            value["identity"]
                .as_str()
                .map(str::to_owned)
                .ok_or("missing identity".to_string())
        };
        let items = |field: &str| {
            value[field]
                .as_array()
                .ok_or("missing runtime elements".to_string())?
                .iter()
                .map(Self::from_value)
                .collect::<Result<Vec<_>, _>>()
        };
        match kind {
            "atom" => Ok(Self::atom(value["value"].as_str().ok_or("missing atom")?)),
            "integer" => Ok(Self::integer(
                BigInt::from_str(value["value"].as_str().ok_or("missing integer")?)
                    .map_err(|e| e.to_string())?,
            )),
            "float" => Self::float(value["value"].as_f64().ok_or("missing finite float")?),
            "bitstring" => Self::bitstring(
                BigInt::from_str(value["value"].as_str().unwrap_or("0"))
                    .map_err(|e| e.to_string())?,
                value["bits"]
                    .as_u64()
                    .ok_or("missing bit size")?
                    .try_into()
                    .map_err(|_| "bit size exceeds host size")?,
            ),
            "function" => Self::function(
                identity()?,
                value["arity"]
                    .as_u64()
                    .ok_or("missing arity")?
                    .try_into()
                    .map_err(|_| "arity exceeds host size")?,
            ),
            "pid" => Ok(Self::pid(identity()?)),
            "port" => Ok(Self::port(identity()?)),
            "reference" => Ok(Self::reference(identity()?)),
            "nil" => Ok(Self::nil()),
            "cons" => Ok(Self::cons(
                Self::from_value(&value["head"])?,
                Self::from_value(&value["tail"])?,
            )),
            "tuple" => Ok(Self::tuple(items("elements")?)),
            #[cfg(otp_domain = "29")]
            "record" | "native_record" => {
                let mut fields = BTreeMap::new();
                for field in value["fields"].as_array().ok_or("missing runtime record fields")? {
                    let key = field["key"].as_str().ok_or("runtime record field must be atom")?.to_owned();
                    if fields.insert(key, Self::from_value(&field["value"])?).is_some() {
                        return Err("duplicate runtime record field".into());
                    }
                }
                Ok(Self::record(value["module"].as_str().ok_or("missing runtime record module")?,
                    value["name"].as_str().ok_or("missing runtime record name")?, fields))
            },
            "map" => Ok(Self::map(
                value["pairs"]
                    .as_array()
                    .ok_or("missing runtime map pairs")?
                    .iter()
                    .map(|pair| Ok((Self::from_value(&pair[0])?, Self::from_value(&pair[1])?)))
                    .collect::<Result<Vec<_>, String>>()?,
            )),
            _ => Err("unsupported runtime term kind".into()),
        }
    }
}
impl Type {
    pub fn from_runtime_term(term: &RuntimeTerm) -> Result<Self, LatticeError> {
        Ok(match &term.0 {
            Term::Atom(a) => Self::atom(a),
            Term::Integer(n) => Self::integer(n.clone()),
            Term::Float(_) => Self::float(),
            Term::Bitstring(_, bits) => {
                Self::bitstring(0.into(), BigInt::from(*bits)).map_err(|_| LatticeError::BadArg)?
            }
            Term::Function(_, arity) => {
                Self::function(Some(vec![Self::any(); *arity]), Self::any())
            }
            Term::Pid(_) => Self::builtin(BuiltinType::Pid)?,
            Term::Port(_) => Self::builtin(BuiltinType::Port)?,
            Term::Reference(_) => Self::builtin(BuiltinType::Reference)?,
            Term::Nil => Self::nil(),
            Term::Cons(head, tail) => Self::cons(
                Self::from_runtime_term(head)?,
                Self::from_runtime_term(tail)?,
            )?,
            #[cfg(otp_domain = "29")]
            Term::Record { .. } => Self::record_any(),
            Term::Tuple(elements) => Self::tuple(
                elements
                    .iter()
                    .map(Self::from_runtime_term)
                    .collect::<Result<Vec<_>, _>>()?,
            )?,
            Term::Map(pairs) => {
                let mut entries = Vec::new();
                let mut dk = Self::none();
                let mut dv = Self::none();
                for (key, value) in pairs {
                    let key = Self::from_runtime_term(key)?;
                    let value = Self::from_runtime_term(value)?;
                    if key.predicate(TypePredicate::Singleton)? {
                        entries.push(MapTypeEntry {
                            key,
                            presence: Presence::Mandatory,
                            value,
                        });
                    } else {
                        dk = key.join(&dk)?;
                        dv = value.join(&dv)?;
                    }
                }
                entries.sort_by(|a, b| a.key.native_record_order(&b.key));
                Self::map_types(entries, dk, dv)?
            }
        })
    }
}
