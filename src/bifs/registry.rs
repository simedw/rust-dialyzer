use crate::bifs::arithmetic;
use crate::bifs::boundary::NativeTerm;
use crate::bifs::{predicates, structural};
use crate::lattice::Type;
use num_traits::ToPrimitive;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub struct Mfa {
    pub module: crate::atom::Atom,
    pub function: crate::atom::Atom,
    pub arity: usize,
}
#[derive(Clone, Debug)]
pub struct TransferOutcome {
    pub known: bool,
    pub runtime_builtin: bool,
    pub accepted: Vec<Type>,
    pub result: Type,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransferError {
    Unsupported { mfa: Mfa, family: String },
    InvalidRequest(String),
    NativeInvalidOperation(String),
    NativeException(crate::native_error::NativeTransferError),
}
impl std::fmt::Display for TransferError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TransferError {}
#[derive(Clone, Copy, Debug)]
enum Transfer {
    Arithmetic(&'static str),
    Compare(&'static str),
    Predicate(&'static str),
    FunctionArity,
    Constant,
    SystemInfo,
    Flatten,
    SpecialList(&'static str),
    Callback(&'static str),
    Apply,
    Any,
    MapMerge,
    MapGet,
    MapKey,
    MapPut,
    MapRemove,
    MapUpdate,
    MapSize,
    MapFromKeys,
    MapFromList,
    Record,
    Boolean,
    Bottom,
    Raise,
    Float,
    Integer,
    Nonnegative,
    Atom,
    Pid,
    Binary,
    Identity(usize),
    Join,
    Append,
    Subtract,
    Head,
    Tail,
    Element,
    SetElement,
    TupleList,
    MakeTuple,
    MakeFun,
    ListElements(usize),
    Delete,
    Member,
    NthTail,
    Split,
    Unsupported(&'static str),
}
struct Entry {
    domains: Result<Vec<Type>, String>,
    transfer: Transfer,
    clips: bool,
    constant: Result<Type, String>,
}
pub struct Registry {
    entries: BTreeMap<Mfa, Entry>,
    runtime_bifs: BTreeSet<Mfa>,
    system_selectors: BTreeMap<String, Type>,
}
#[derive(Deserialize)]
struct Catalogue {
    entries: Vec<CatalogueEntry>,
    runtime_builtin_inventory: RuntimeBuiltins,
    system_info_selector_returns: Vec<SelectorReturn>,
}
#[derive(Deserialize)]
struct SelectorReturn {
    selector: String,
    #[serde(rename = "return")]
    result: Descriptor,
}
#[derive(Deserialize)]
struct RuntimeBuiltins {
    mfas: Vec<Mfa>,
}
#[derive(Deserialize)]
struct CatalogueEntry {
    mfa: Mfa,
    native_is_known: bool,
    admissible_domains: Domains,
    clips_to_admissible_domains: bool,
    return_on_any: Descriptor,
}
#[derive(Deserialize)]
struct Domains {
    status: String,
    #[serde(default)]
    arguments: Vec<Descriptor>,
}
#[derive(Deserialize)]
struct Descriptor {
    native: NativeTerm,
}

fn transfer(mfa: &Mfa) -> Transfer {
    use Transfer::*;
    match (mfa.module.as_str(), mfa.function.as_str(), mfa.arity) {
        ("erlang", name, _)
            if [
                "+", "-", "*", "/", "div", "rem", "band", "bor", "bxor", "bsl", "bsr", "bnot",
                "abs",
            ]
            .contains(&name) =>
        {
            Arithmetic(match name {
                "+" => "+",
                "-" => "-",
                "*" => "*",
                "/" => "/",
                "div" => "div",
                "rem" => "rem",
                "band" => "band",
                "bor" => "bor",
                "bxor" => "bxor",
                "bsl" => "bsl",
                "bsr" => "bsr",
                "bnot" => "bnot",
                _ => "abs",
            })
        }
        ("erlang", name, 2) if ["==", "/=", "=:=", "=/=", "<", ">", "=<", ">="].contains(&name) => {
            Compare(match name {
                "==" => "==",
                "/=" => "/=",
                "=:=" => "=:=",
                "=/=" => "=/=",
                "<" => "<",
                ">" => ">",
                "=<" => "=<",
                _ => ">=",
            })
        }
        ("erlang", name, 1)
            if [
                "is_atom",
                "is_float",
                "is_integer",
                "is_number",
                "is_boolean",
                "is_binary",
                "is_bitstring",
                "is_function",
                "is_list",
                "is_tuple",
                "is_pid",
                "is_port",
                "is_reference",
            ]
            .contains(&name) =>
        {
            Predicate(match name {
                "is_atom" => "is_atom",
                "is_float" => "is_float",
                "is_integer" => "is_integer",
                "is_number" => "is_number",
                "is_boolean" => "is_boolean",
                "is_binary" => "is_binary",
                "is_bitstring" => "is_bitstring",
                "is_function" => "is_function",
                "is_list" => "is_list",
                "is_tuple" => "is_tuple",
                "is_pid" => "is_pid",
                "is_port" => "is_port",
                _ => "is_reference",
            })
        }
        ("erlang", "is_function", 2) => FunctionArity,
        ("erlang", "is_record", _) => Record,
        ("erlang", "and" | "or" | "xor" | "not", _) => Boolean,
        ("erlang", "raise", 3) => Raise,
        ("erlang", "halt" | "exit" | "error" | "throw", _) => Bottom,
        ("erlang", "float", 1) => Float,
        ("erlang", "ceil" | "floor" | "round" | "trunc", 1) => Integer,
        ("erlang", "bit_size" | "byte_size" | "length" | "tuple_size" | "size", 1) => Nonnegative,
        ("erlang", "node", _) => Atom,
        ("erlang", "self", 0) => Pid,
        ("erlang", "binary_part", _) => Binary,
        ("erlang", "!", 2) => Identity(1),
        ("erlang", "max" | "min", 2) => Join,
        ("erlang", "++" | "append", 2) | ("lists", "append" | "reverse", 2) => Append,
        ("erlang", "--" | "subtract", 2) | ("lists", "subtract", 2) => Subtract,
        ("erlang", "hd", 1) => Head,
        ("erlang", "tl", 1) => Tail,
        ("erlang", "element", 2) => Element,
        ("erlang", "setelement", 3) => SetElement,
        ("erlang", "tuple_to_list", 1) => TupleList,
        ("erlang", "make_tuple", _) => MakeTuple,
        ("erlang", "make_fun", 3) => MakeFun,
        ("lists", "last" | "max" | "min", 1) => ListElements(0),
        ("lists", "nth", 2) => ListElements(1),
        ("lists", "delete", 2) => Delete,
        ("lists", "member", 2) => Member,
        ("lists", "nthtail", 2) => NthTail,
        ("lists", "reverse" | "sort" | "usort", 1) => Identity(0),
        ("lists", "keysort", 2) => Identity(1),
        ("lists", "merge", 2) => Join,
        ("lists", "split", 2) => Split,
        ("maps", "merge", 2) => MapMerge,
        ("maps", "get", 2) | ("erlang", "map_get", 2) => MapGet,
        ("maps", "is_key", 2) | ("erlang", "is_map_key", 2) => MapKey,
        ("maps", "size", 1) | ("erlang", "map_size", 1) => MapSize,
        ("maps", "put", 3) => MapPut,
        ("maps", "remove", 2) => MapRemove,
        ("maps", "update", 3) => MapUpdate,
        ("maps", "from_keys", 2) => MapFromKeys,
        ("maps", "from_list", 1) => MapFromList,
        ("erlang", "is_map", 1) => Predicate("is_map"),
        ("maps", _, _) => Unsupported("canonical map domain"),
        ("erlang", "build_stacktrace", 0) => Constant,
        ("erlang", "system_info" | "info", 1) => SystemInfo,
        ("erlang", "apply", 2) => Apply,
        ("erlang", "apply", 3) => Any,
        ("erlang", "nif_error", _) => Any,
        ("lists", name, _)
            if [
                "dropwhile",
                "takewhile",
                "splitwith",
                "all",
                "any",
                "foreach",
                "foldl",
                "foldr",
                "map",
                "flatmap",
                "filter",
                "partition",
                "mapfoldl",
                "mapfoldr",
                "sort",
                "usort",
            ]
            .contains(&name) =>
        {
            Callback(match name {
                "dropwhile" => "dropwhile",
                "takewhile" => "takewhile",
                "splitwith" => "splitwith",
                "all" => "all",
                "any" => "any",
                "foreach" => "foreach",
                "foldl" => "foldl",
                "foldr" => "foldr",
                "map" => "map",
                "flatmap" => "flatmap",
                "filter" => "filter",
                "partition" => "partition",
                "mapfoldl" => "mapfoldl",
                "mapfoldr" => "mapfoldr",
                "sort" => "sort",
                _ => "usort",
            })
        }
        ("lists", name, _)
            if [
                "zip",
                "zip3",
                "zipwith",
                "zipwith3",
                "unzip",
                "unzip3",
                "keydelete",
                "keyreplace",
                "keymap",
                "keymerge",
                "keyfind",
                "keymember",
                "keysearch",
            ]
            .contains(&name) =>
        {
            SpecialList(match name {
                "zip" => "zip",
                "zip3" => "zip3",
                "zipwith" => "zipwith",
                "zipwith3" => "zipwith3",
                "unzip" => "unzip",
                "unzip3" => "unzip3",
                "keydelete" => "keydelete",
                "keyreplace" => "keyreplace",
                "keymap" => "keymap",
                "keymerge" => "keymerge",
                "keyfind" => "keyfind",
                "keymember" => "keymember",
                _ => "keysearch",
            })
        }
        ("lists", "flatten", 1) => Flatten,
        ("lists", _, _) => Unsupported("higher-order or specialized list transfer"),
        _ => Unsupported("specialized transfer not implemented"),
    }
}
impl Registry {
    pub fn load(text: &str) -> Result<Self, String> {
        let catalogue: Catalogue = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut entries = BTreeMap::new();
        for record in catalogue.entries {
            if !record.native_is_known || record.admissible_domains.status != "known" {
                return Err("catalogue specialized entry lacks known domains".into());
            }
            let domains = record
                .admissible_domains
                .arguments
                .into_iter()
                .map(|x| x.native.type_value())
                .collect::<Result<Vec<_>, _>>();
            if let Ok(args) = &domains
                && args.len() != record.mfa.arity
            {
                return Err("catalogue argument arity mismatch".into());
            }
            let entry = Entry {
                domains,
                transfer: transfer(&record.mfa),
                clips: record.clips_to_admissible_domains,
                constant: record.return_on_any.native.type_value(),
            };
            if entries.insert(record.mfa, entry).is_some() {
                return Err("duplicate catalogue MFA".into());
            }
        }
        let system_selectors = catalogue
            .system_info_selector_returns
            .into_iter()
            .map(|s| Ok((s.selector, s.result.native.type_value()?)))
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        Ok(Self {
            entries,
            system_selectors,
            runtime_bifs: catalogue
                .runtime_builtin_inventory
                .mfas
                .into_iter()
                .collect(),
        })
    }
    pub fn is_known(&self, mfa: &Mfa) -> bool {
        self.entries.contains_key(mfa)
    }
    pub fn is_runtime_builtin(&self, mfa: &Mfa) -> bool {
        self.runtime_bifs.contains(mfa)
    }
    pub fn admissible_domains(&self, mfa: &Mfa) -> Result<Option<Vec<Type>>, TransferError> {
        if let Some(entry) = self.entries.get(mfa) {
            entry
                .domains
                .as_ref()
                .cloned()
                .map(Some)
                .map_err(|e| TransferError::Unsupported {
                    mfa: mfa.clone(),
                    family: e.clone(),
                })
        } else {
            Ok(None)
        }
    }
    pub fn argument_domains(&self, mfa: &Mfa) -> Result<Vec<Type>, TransferError> {
        if let Some(entry) = self.entries.get(mfa) {
            entry
                .domains
                .as_ref()
                .cloned()
                .map_err(|e| TransferError::Unsupported {
                    mfa: mfa.clone(),
                    family: e.clone(),
                })
        } else if self.is_runtime_builtin(mfa) {
            Ok(vec![Type::any(); mfa.arity])
        } else {
            Err(TransferError::Unsupported {
                mfa: mfa.clone(),
                family: "missing library summary".into(),
            })
        }
    }
    pub fn supported_mfas(&self) -> Vec<Mfa> {
        self.entries
            .iter()
            .filter(|(_, x)| x.domains.is_ok() && !matches!(x.transfer, Transfer::Unsupported(_)))
            .map(|(m, _)| m.clone())
            .collect()
    }
    pub fn transfer(&self, mfa: &Mfa, args: &[Type]) -> Result<TransferOutcome, TransferError> {
        if mfa.arity != args.len() || mfa.arity > 255 {
            return Err(TransferError::InvalidRequest(
                "argument arity mismatch".into(),
            ));
        }
        let Some(entry) = self.entries.get(mfa) else {
            if !self.is_runtime_builtin(mfa) {
                return Err(TransferError::Unsupported {
                    mfa: mfa.clone(),
                    family: "missing library summary".into(),
                });
            }
            return Ok(TransferOutcome {
                known: false,
                runtime_builtin: true,
                accepted: args.to_vec(),
                result: if args.iter().any(Type::is_impossible) {
                    Type::none()
                } else {
                    Type::any()
                },
            });
        };
        let unsupported = |family: String| TransferError::Unsupported {
            mfa: mfa.clone(),
            family,
        };
        let domains = entry.domains.as_ref().map_err(|e| unsupported(e.clone()))?;
        if let Transfer::Unsupported(family) = entry.transfer {
            return Err(unsupported(family.into()));
        }
        let accepted = args
            .iter()
            .zip(domains)
            .map(|(a, d)| {
                a.meet(d)
                    .map_err(|e| TransferError::NativeException(e.into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let values = if entry.clips { &accepted[..] } else { args };
        let result = if matches!(entry.transfer, Transfer::Raise) {
            if accepted.iter().any(Type::is_impossible) {
                Type::atom("badarg")
            } else {
                Type::none()
            }
        } else if matches!(entry.transfer, Transfer::Bottom)
            || values.iter().any(Type::is_impossible)
        {
            Type::none()
        } else {
            use Transfer::*;
            let native = TransferError::NativeException;
            match entry.transfer {
                Arithmetic(op) => if values.len() == 1 {
                    arithmetic::unary(op, &values[0])
                } else {
                    arithmetic::binary(op, &values[0], &values[1])
                }
                .map_err(native)?,
                Compare(op) => predicates::compare(op, &values[0], &values[1]).map_err(native)?,
                Predicate(name) => {
                    predicates::test(&values[0], &predicates::class(name).map_err(native)?)
                        .map_err(native)?
                }
                FunctionArity => {
                    predicates::function_arity(&values[0], &values[1]).map_err(native)?
                }
                Record => structural::record(&values[0], &values[1], values.get(2))
                    .map_err(TransferError::NativeException)?,
                Constant => entry
                    .constant
                    .as_ref()
                    .map_err(|e| native(e.clone().into()))?
                    .clone(),
                SystemInfo => match predicates::atom_values(&values[0]).map_err(native)? {
                    Some(values) if values.len() == 1 => self
                        .system_selectors
                        .get(&values[0])
                        .cloned()
                        .unwrap_or_else(Type::any),
                    _ => Type::any(),
                },
                Flatten => crate::bifs::list_special::flatten(&values[0]).map_err(native)?,
                SpecialList(name) => {
                    crate::bifs::list_special::transfer(name, values).map_err(native)?
                }
                Callback(name) => crate::bifs::callbacks::transfer(name, values).map_err(native)?,
                Apply => {
                    if values[0]
                        .is_subtype(&Type::function(None, Type::any()))
                        .map_err(|e| native(e.into()))?
                    {
                        crate::bifs::callbacks::range(&values[0]).map_err(native)?
                    } else {
                        Type::any()
                    }
                }
                Any => Type::any(),
                MapMerge => crate::bifs::maps::merge(&values[0], &values[1]).map_err(native)?,
                MapGet => values[1]
                    .map_get(&values[0])
                    .map_err(|e| native(e.into()))?,
                MapKey => values[1]
                    .map_is_key(&values[0])
                    .map_err(|e| native(e.into()))?,
                MapPut => values[2]
                    .map_put(values[0].clone(), values[1].clone())
                    .map_err(|e| native(e.into()))?,
                MapRemove => values[1]
                    .map_remove(&values[0])
                    .map_err(|e| native(e.into()))?,
                MapUpdate => values[2]
                    .map_update(values[0].clone(), values[1].clone())
                    .map_err(|e| native(e.into()))?,
                MapSize => crate::bifs::maps::size(&values[0]).map_err(native)?,
                MapFromKeys => {
                    crate::bifs::maps::from_keys(&values[0], &values[1]).map_err(native)?
                }
                MapFromList => crate::bifs::maps::from_list(&values[0]).map_err(native)?,
                Boolean => predicates::boolean(),
                Float => Type::float(),
                Integer => Type::all_integers(),
                Nonnegative => Type::range_strings("0", "pos_inf").map_err(|e| native(e.into()))?,
                Atom => Type::all_atoms(),
                Pid => Type::identifier(Some(vec!["pid".into()])).map_err(|e| native(e.into()))?,
                Binary => Type::binary(),
                Identity(i) => values[i].clone(),
                Join => values[0]
                    .join(&values[1])
                    .map_err(|e| TransferError::NativeException(e.into()))?,
                Append => structural::append(&values[0], &values[1]).map_err(native)?,
                Subtract => structural::subtract(&values[0], &values[1])
                    .map_err(TransferError::NativeException)?,
                Head => {
                    structural::list_elements(&values[0]).map_err(TransferError::NativeException)?
                }
                Tail => structural::tail(&values[0]).map_err(native)?,
                Element => structural::element(&values[0], &values[1]).map_err(native)?,
                SetElement => {
                    structural::setelement(&values[0], &values[1], &values[2]).map_err(native)?
                }
                TupleList => structural::tuple_list(&values[0]).map_err(native)?,
                MakeTuple => {
                    structural::make_tuple(&values[0]).map_err(TransferError::NativeException)?
                }
                MakeFun => {
                    match arithmetic::values(&values[2]).map_err(TransferError::NativeException)? {
                        Some(v) if v.len() == 1 => match v[0].to_usize() {
                            Some(n) if n <= 255 => {
                                Type::function(Some(vec![Type::any(); n]), Type::any())
                            }
                            _ => Type::none(),
                        },
                        _ => Type::function(None, Type::any()),
                    }
                }
                ListElements(i) => {
                    structural::list_elements(&values[i]).map_err(TransferError::NativeException)?
                }
                Delete => {
                    if structural::nonempty(&values[1]).map_err(TransferError::NativeException)? {
                        structural::tail(&values[1]).map_err(native)?
                    } else {
                        values[1].clone()
                    }
                }
                Member => structural::member(&values[0], &values[1]).map_err(native)?,
                NthTail => values[1]
                    .join(&Type::proper_list(Type::any(), false))
                    .map_err(|e| TransferError::NativeException(e.into()))?,
                Split => {
                    let list = values[1].clone();
                    let left = if structural::is_nil(&list).map_err(native)? {
                        Type::nil()
                    } else {
                        Type::proper_list(
                            structural::list_elements(&list)
                                .map_err(TransferError::NativeException)?,
                            false,
                        )
                    };
                    Type::tuple(vec![left.clone(), left])
                        .map_err(|e| TransferError::NativeException(e.into()))?
                }
                Bottom | Raise | Unsupported(_) => unreachable!(),
            }
        };
        Ok(TransferOutcome {
            known: true,
            runtime_builtin: self.is_runtime_builtin(mfa),
            accepted,
            result,
        })
    }
    pub fn predicate_domain(
        &self,
        mfa: &Mfa,
        args: &[Type],
    ) -> Result<Option<Type>, TransferError> {
        Ok(match self.entries.get(mfa).map(|x| x.transfer) {
            Some(Transfer::Predicate(name)) => {
                Some(predicates::class(name).map_err(TransferError::NativeException)?)
            }
            Some(Transfer::FunctionArity) => arithmetic::values(&args[1])
                .map_err(TransferError::NativeException)?
                .and_then(|v| if v.len() == 1 { v[0].to_usize() } else { None })
                .map(|n| Type::function(Some(vec![Type::any(); n]), Type::any())),
            _ => None,
        })
    }
    pub fn guard_refinements(
        &self,
        mfa: &Mfa,
        args: &[Type],
        truth: bool,
    ) -> Result<Option<Vec<Type>>, TransferError> {
        if mfa.arity != args.len() {
            return Err(TransferError::InvalidRequest("guard arity mismatch".into()));
        }
        if mfa.module == "erlang" && mfa.function == "is_record" && mfa.arity == 3 {
            let tags = predicates::atom_values(&args[1]).map_err(TransferError::NativeException)?;
            let arities = arithmetic::values(&args[2]).map_err(TransferError::NativeException)?;
            if let (Some(tags), Some(arities)) = (tags, arities)
                && tags.len() == 1
                && arities.len() == 1
                && let Some(arity) = arities[0].to_usize().filter(|arity| *arity > 0)
            {
                let mut fields = vec![Type::any(); arity];
                fields[0] = Type::atom(tags[0].clone());
                let expected =
                    Type::tuple(fields).map_err(|e| TransferError::NativeException(e.into()))?;
                let mut refined = args.to_vec();
                if truth {
                    refined[0] = refined[0]
                        .meet(&expected)
                        .map_err(|e| TransferError::NativeException(e.into()))?;
                }
                return Ok(Some(refined));
            }
        }
        if let Some(domain) = self.predicate_domain(mfa, args)? {
            let mut refined = args.to_vec();
            refined[0] = if truth {
                args[0]
                    .meet(&domain)
                    .map_err(|e| TransferError::NativeException(e.into()))?
            } else {
                args[0]
                    .subtract(&domain)
                    .map_err(|e| TransferError::NativeException(e.into()))?
            };
            return Ok(Some(refined));
        }
        if mfa.arity != args.len() {
            return Err(TransferError::InvalidRequest("guard arity mismatch".into()));
        }
        if mfa.module == "erlang" && ["<", "=<", ">", ">="].contains(&mfa.function.as_str()) {
            return predicates::ordering_refinement(&mfa.function, &args[0], &args[1], truth)
                .map_err(TransferError::NativeException);
        }
        if mfa.module == "erlang"
            && ((mfa.function == "=:=" && truth) || (mfa.function == "=/=" && !truth))
        {
            let intersection = args[0]
                .meet(&args[1])
                .map_err(|e| TransferError::NativeException(e.into()))?;
            return Ok(Some(vec![intersection.clone(), intersection]));
        }
        Ok(None)
    }
}
