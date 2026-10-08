//! Interned atom names.
//!
//! Every distinct name is stored once for the life of the process (the BEAM
//! never frees atoms either; an analysis sees tens of thousands). An `Atom` is
//! a copyable pointer to that single copy, so equality and hashing compare
//! pointers, while ordering compares names, which is Erlang's atom order.

use std::cell::RefCell;
use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy)]
pub struct Atom(&'static str);

fn global() -> &'static Mutex<HashSet<&'static str>> {
    static TABLE: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    TABLE.get_or_init(Default::default)
}

thread_local! {
    /// Per-thread front for the global table, so hot lookups take no lock.
    static LOCAL: RefCell<crate::fast_hash::FastHashSet<&'static str>> = RefCell::default();
}

impl Atom {
    pub fn new(name: &str) -> Self {
        if let Some(found) = LOCAL.with(|local| local.borrow().get(name).copied()) {
            return Atom(found);
        }
        let interned = {
            let mut table = global().lock().expect("atom table");
            match table.get(name) {
                Some(existing) => *existing,
                None => {
                    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
                    table.insert(leaked);
                    leaked
                }
            }
        };
        LOCAL.with(|local| local.borrow_mut().insert(interned));
        Atom(interned)
    }
    pub fn as_str(self) -> &'static str {
        self.0
    }
}

impl PartialEq for Atom {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.0, other.0)
    }
}
impl Eq for Atom {}
impl Hash for Atom {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.0.as_ptr() as usize).hash(state)
    }
}
impl Ord for Atom {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self == other {
            std::cmp::Ordering::Equal
        } else {
            self.0.cmp(other.0)
        }
    }
}
impl PartialOrd for Atom {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl std::ops::Deref for Atom {
    type Target = str;
    fn deref(&self) -> &str {
        self.0
    }
}
impl AsRef<str> for Atom {
    fn as_ref(&self) -> &str {
        self.0
    }
}
/// Prints like `String`, so debug output of types is unchanged.
impl fmt::Debug for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.0, f)
    }
}
impl fmt::Display for Atom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}
impl PartialEq<str> for Atom {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}
impl PartialEq<&str> for Atom {
    fn eq(&self, other: &&str) -> bool {
        self.0 == *other
    }
}
impl PartialEq<String> for Atom {
    fn eq(&self, other: &String) -> bool {
        self.0 == other.as_str()
    }
}
impl From<&str> for Atom {
    fn from(name: &str) -> Self {
        Atom::new(name)
    }
}
impl From<&String> for Atom {
    fn from(name: &String) -> Self {
        Atom::new(name)
    }
}
impl From<&Atom> for Atom {
    fn from(atom: &Atom) -> Self {
        *atom
    }
}
impl From<String> for Atom {
    fn from(name: String) -> Self {
        Atom::new(&name)
    }
}
impl From<&Atom> for String {
    fn from(atom: &Atom) -> Self {
        atom.0.to_owned()
    }
}
impl serde::Serialize for Atom {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.0)
    }
}
impl<'de> serde::Deserialize<'de> for Atom {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = <std::borrow::Cow<'de, str>>::deserialize(deserializer)?;
        Ok(Atom::new(&name))
    }
}
impl From<Atom> for String {
    fn from(atom: Atom) -> Self {
        atom.0.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::Atom;

    #[test]
    fn interning_shares_storage_and_orders_by_name() {
        let a = Atom::new("beta");
        let b = Atom::new(&String::from("beta"));
        assert_eq!(a, b);
        assert!(std::ptr::eq(a.as_str(), b.as_str()));
        assert!(Atom::new("alpha") < a);
        assert_eq!(format!("{a:?}"), format!("{:?}", "beta".to_string()));
        let from_other_thread = std::thread::spawn(|| Atom::new("beta")).join().unwrap();
        assert_eq!(from_other_thread, a);
    }
}
