//! Interned atom names.
//!
//! Every distinct name is stored once for the life of the process (the BEAM
//! never frees atoms either; an analysis sees tens of thousands). An `Atom` is
//! a copyable pointer to that single copy, so equality and hashing compare
//! pointers, while ordering compares names, which is Erlang's atom order.

use std::collections::HashSet;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Mutex;
use std::sync::atomic::{AtomicPtr, Ordering};

#[derive(Clone, Copy)]
pub struct Atom(&'static str);

/// Open-addressing set whose slots are only ever filled, never cleared or
/// moved, so lookups are plain atomic loads and threads never wait on each
/// other. A name lives in the first `PROBES` slots of its chain or, once all
/// of those hold other names (which then stays true forever), in `overflow`.
struct Table<const SLOTS: usize> {
    slots: [AtomicPtr<&'static str>; SLOTS],
    overflow: Mutex<Option<HashSet<&'static str>>>,
}

const PROBES: usize = 32;

impl<const SLOTS: usize> Table<SLOTS> {
    const fn new() -> Self {
        Table {
            slots: [const { AtomicPtr::new(std::ptr::null_mut()) }; SLOTS],
            overflow: Mutex::new(None),
        }
    }

    fn intern(&self, name: &str) -> &'static str {
        let mut hasher = crate::fast_hash::FxHasher::default();
        hasher.write(name.as_bytes());
        hasher.write_usize(name.len());
        let start = (hasher.finish() >> 32) as usize;
        let mut fresh: Option<*mut &'static str> = None;
        for probe in 0..PROBES.min(SLOTS) {
            let slot = &self.slots[(start + probe) % SLOTS];
            let mut current = slot.load(Ordering::Acquire);
            if current.is_null() {
                let entry = *fresh.get_or_insert_with(|| {
                    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
                    Box::into_raw(Box::new(leaked))
                });
                match slot.compare_exchange(std::ptr::null_mut(), entry, Ordering::AcqRel, Ordering::Acquire) {
                    Ok(_) => return unsafe { *entry },
                    Err(winner) => current = winner,
                }
            }
            // Published entries are leaked, so they stay valid forever.
            let existing = unsafe { *current };
            if existing == name {
                if let Some(entry) = fresh {
                    // Never published: reclaim both allocations.
                    let unused = unsafe { Box::from_raw(entry) };
                    drop(unsafe { Box::from_raw(*unused as *const str as *mut str) });
                }
                return existing;
            }
        }
        let mut overflow = self.overflow.lock().expect("atom overflow table");
        let overflow = overflow.get_or_insert_with(HashSet::new);
        match overflow.get(name) {
            Some(existing) => existing,
            None => {
                let leaked: &'static str = match fresh {
                    Some(entry) => unsafe { *Box::from_raw(entry) },
                    None => Box::leak(name.to_owned().into_boxed_str()),
                };
                overflow.insert(leaked);
                leaked
            }
        }
    }
}

/// 8 MB of zeroed static memory, touched only where atoms land; an analysis
/// interns at most a few hundred thousand names, far below the chain limit.
static TABLE: Table<{ 1 << 20 }> = Table::new();

impl Atom {
    pub fn new(name: &str) -> Self {
        Atom(TABLE.intern(name))
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

    #[test]
    fn racing_threads_share_one_copy_including_overflow() {
        // Four slots fill quickly, so most names take the overflow path.
        static SMALL: super::Table<4> = super::Table::new();
        let names: Vec<String> = (0..64).map(|i| format!("name{i}")).collect();
        let runs: Vec<Vec<usize>> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| scope.spawn(|| names.iter().map(|n| SMALL.intern(n).as_ptr() as usize).collect()))
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        for run in &runs {
            assert_eq!(run, &runs[0]);
        }
        for (name, ptr) in names.iter().zip(&runs[0]) {
            assert_eq!(SMALL.intern(name).as_ptr() as usize, *ptr);
            assert_eq!(SMALL.intern(name), name);
        }
    }
}
