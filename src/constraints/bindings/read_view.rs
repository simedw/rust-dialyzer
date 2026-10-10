use super::{BindingAccess, BindingKey, BindingMap};
use crate::fast_hash::FxHasher;
use crate::lattice::Type;
use std::cell::OnceCell;
use std::hash::Hasher;

const READ_SLOTS: usize = 16;
struct ReadEntry {
    index: u64,
    value: Option<Type>,
}
struct ReadBank {
    cells: Box<[OnceCell<ReadEntry>]>,
}
enum BankState {
    Disabled,
    Ready(ReadBank),
}
pub(in crate::constraints) struct ReadView {
    backing: BindingMap,
    bank: OnceCell<BankState>,
}
fn slot(index: u64) -> usize {
    let mut hasher = FxHasher::default();
    hasher.write_u64(index);
    let hash = hasher.finish();
    ((hash ^ (hash >> 32)) as usize) & (READ_SLOTS - 1)
}
impl ReadBank {
    fn allocate() -> BankState {
        #[cfg(test)]
        record(|d| d.allocation_attempts += 1);
        #[cfg(test)]
        if REFUSAL.get() == 1 {
            record(|d| d.allocation_refusals += 1);
            return BankState::Disabled;
        }
        let mut cells = Vec::new();
        if cells.try_reserve_exact(READ_SLOTS).is_err() {
            #[cfg(test)]
            record(|d| d.allocation_refusals += 1);
            return BankState::Disabled;
        }
        #[cfg(test)]
        record(|d| {
            d.reserved_bytes += cells.capacity() * std::mem::size_of::<OnceCell<ReadEntry>>()
        });
        let capacity_matches = cells.capacity() == READ_SLOTS;
        #[cfg(test)]
        let capacity_matches = capacity_matches && REFUSAL.get() != 2;
        if !capacity_matches {
            #[cfg(test)]
            record(|d| d.allocation_refusals += 1);
            return BankState::Disabled;
        }
        cells.resize_with(READ_SLOTS, OnceCell::new);
        // Exact length == capacity, so this conversion does not shrink/reallocate.
        let cells = cells.into_boxed_slice();
        #[cfg(test)]
        record(|d| {
            d.banks_initialized += 1;
            d.cells_initialized += READ_SLOTS;
        });
        BankState::Ready(Self { cells })
    }
}
impl ReadView {
    pub(in crate::constraints) fn new(backing: BindingMap) -> Self {
        Self {
            backing,
            bank: OnceCell::new(),
        }
    }
    pub(in crate::constraints) fn into_native(self) -> BindingMap {
        let Self { backing, bank } = self;
        drop(bank);
        backing
    }
    fn raw(&self, key: &BindingKey) -> Option<&Type> {
        #[cfg(test)]
        record(|d| {
            if key.trie_index().is_some() {
                d.raw_trie_reads += 1;
            } else {
                d.raw_rest_reads += 1;
            }
        });
        self.backing.raw_get(key)
    }
    fn visible(&self, key: &BindingKey) -> Option<&Type> {
        let Some(index) = key.trie_index() else {
            #[cfg(test)]
            record(|d| d.inadmissible_reads += 1);
            return self.raw(key);
        };
        let BankState::Ready(bank) = self.bank.get_or_init(ReadBank::allocate) else {
            #[cfg(test)]
            record(|d| d.disabled_reads += 1);
            return self.raw(key);
        };
        let cell = &bank.cells[slot(index)];
        if let Some(entry) = cell.get() {
            if entry.index != index {
                #[cfg(test)]
                record(|d| d.collisions += 1);
                return self.raw(key);
            }
            #[cfg(test)]
            record(|d| {
                if entry.value.is_some() {
                    d.positive_hits += 1;
                } else {
                    d.negative_hits += 1;
                }
            });
            if crate::lattice::verify_fast_paths() {
                #[cfg(test)]
                record(|d| d.verify_raw_reads += 1);
                assert_eq!(
                    entry.value.as_ref(),
                    self.backing.raw_get(key),
                    "read view raw hit witness"
                );
            }
            return entry.value.as_ref();
        }
        cell.get_or_init(|| ReadEntry {
            index,
            value: self.raw(key).copied(),
        })
        .value
        .as_ref()
    }
    fn invalidate(&mut self, key: &BindingKey) {
        let Some(index) = key.trie_index() else {
            return;
        };
        let Some(BankState::Ready(bank)) = self.bank.get_mut() else {
            return;
        };
        let cell = &mut bank.cells[slot(index)];
        if cell.get().is_some_and(|entry| entry.index == index) {
            cell.take();
            #[cfg(test)]
            record(|d| d.invalidations += 1);
        }
    }
}
impl BindingAccess for ReadView {
    fn get(&self, key: &BindingKey) -> Option<&Type> {
        #[cfg(test)]
        record(|d| d.logical_reads += 1);
        self.backing.lookup_visible(key, || self.visible(key))
    }
    fn insert(&mut self, key: BindingKey, value: Type) -> Option<Type> {
        self.invalidate(&key);
        self.backing.insert(key, value)
    }
    fn remove(&mut self, key: &BindingKey) -> Option<Type> {
        self.invalidate(key);
        self.backing.remove(key)
    }
    fn native_map(&self) -> &BindingMap {
        &self.backing
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
struct Diagnostics {
    logical_reads: usize,
    raw_trie_reads: usize,
    raw_rest_reads: usize,
    verify_raw_reads: usize,
    positive_hits: usize,
    negative_hits: usize,
    collisions: usize,
    inadmissible_reads: usize,
    disabled_reads: usize,
    invalidations: usize,
    allocation_attempts: usize,
    allocation_refusals: usize,
    reserved_bytes: usize,
    banks_initialized: usize,
    cells_initialized: usize,
}
#[cfg(test)]
thread_local! {
    static DIAGNOSTICS: std::cell::Cell<Diagnostics> = std::cell::Cell::new(Diagnostics::default());
    static REFUSAL: std::cell::Cell<u8> = const { std::cell::Cell::new(0) };
}
#[cfg(test)]
fn record(edit: impl FnOnce(&mut Diagnostics)) {
    DIAGNOSTICS.with(|cell| {
        let mut d = cell.get();
        edit(&mut d);
        cell.set(d);
    });
}
#[cfg(test)]
pub(in crate::constraints) fn layout() -> serde_json::Value {
    use std::mem::{align_of, size_of};
    DIAGNOSTICS.set(Diagnostics::default());
    REFUSAL.set(0);
    let view = ReadView::new(BindingMap::new());
    let key = BindingKey::Label(crate::lattice::VariableId::Integer(0.into()));
    assert_eq!(view.get(&key), None);
    let d = DIAGNOSTICS.get();
    serde_json::json!({
        "read_view_bytes": size_of::<ReadView>(), "read_view_align": align_of::<ReadView>(),
        "bank_state_bytes": size_of::<BankState>(), "read_bank_bytes": size_of::<ReadBank>(),
        "read_entry_bytes": size_of::<ReadEntry>(), "once_cell_bytes": size_of::<OnceCell<ReadEntry>>(),
        "read_slots": READ_SLOTS, "bank_payload_bytes": READ_SLOTS * size_of::<OnceCell<ReadEntry>>(),
        "observed_allocation_attempts": d.allocation_attempts,
        "observed_reserved_bytes": d.reserved_bytes,
        "observed_banks_initialized": d.banks_initialized,
        "observed_cells_initialized": d.cells_initialized
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constraints::atomic::{
        AtomicConstraint, AtomicStatus, ConstraintValue, KeySet, Relation,
    };
    use crate::constraints::bindings::{BindingKey, RootCatalogue};
    use crate::constraints::derived::{DerivedArena, NoBifProvider};
    use crate::lattice::{MapTypeEntry, Presence, VariableId};

    fn key(n: u64) -> BindingKey {
        BindingKey::Label(VariableId::Integer(n.into()))
    }
    fn reset() {
        DIAGNOSTICS.set(Diagnostics::default());
        REFUSAL.set(0);
    }
    fn other_slot(index: u64) -> u64 {
        (0..100).find(|&n| slot(n) != slot(index)).unwrap()
    }
    fn collision(index: u64) -> u64 {
        (0..1000)
            .find(|&n| n != index && slot(n) == slot(index))
            .unwrap()
    }
    #[test]
    fn positive_negative_hits_keep_logical_reads_and_remove_only_raw_descent() {
        reset();
        let positive = 1;
        let negative = other_slot(positive);
        let input = BindingMap::from(vec![(key(positive), Type::atom("value"))]);
        let view = ReadView::new(input.clone());
        assert!(view.bank.get().is_none());
        assert_eq!(view.get(&key(positive)), Some(&Type::atom("value")));
        assert_eq!(view.get(&key(positive)), Some(&Type::atom("value")));
        assert_eq!(view.get(&key(negative)), None);
        assert_eq!(view.get(&key(negative)), None);
        let d = DIAGNOSTICS.get();
        assert_eq!(
            (
                d.logical_reads,
                d.raw_trie_reads,
                d.positive_hits,
                d.negative_hits
            ),
            (4, 2, 1, 1)
        );
        assert_eq!(
            d.verify_raw_reads,
            if crate::lattice::verify_fast_paths() {
                2
            } else {
                0
            }
        );
        assert_eq!((d.banks_initialized, d.cells_initialized), (1, 16));
        assert!(view.into_native().ptr_eq(&input));
    }
    #[test]
    fn exact_key_invalidation_and_collision_fallback_keep_some_none_and_absence_distinct() {
        reset();
        let first = 1;
        let second = collision(first);
        let mut view = ReadView::new(BindingMap::new());
        assert_eq!(view.get(&key(first)), None);
        assert_eq!(view.insert(key(second), Type::atom("collision")), None);
        assert_eq!(view.get(&key(first)), None);
        assert_eq!(view.get(&key(second)), Some(&Type::atom("collision")));
        assert_eq!(view.get(&key(second)), Some(&Type::atom("collision")));
        assert_eq!(DIAGNOSTICS.get().invalidations, 0);
        assert_eq!(DIAGNOSTICS.get().collisions, 2);
        assert_eq!(view.insert(key(first), Type::any()), None);
        assert_eq!(view.get(&key(first)), Some(&Type::any()));
        assert_eq!(view.get(&key(first)), Some(&Type::any()));
        assert_eq!(view.insert(key(first), Type::none()), Some(Type::any()));
        assert_eq!(view.get(&key(first)), Some(&Type::none()));
        assert_eq!(view.remove(&key(first)), Some(Type::none()));
        assert_eq!(view.get(&key(first)), None);
        assert_eq!(view.get(&key(first)), None);
        assert_eq!(view.remove(&key(first)), None);
        assert_eq!(view.insert(key(first), Type::atom("reinsert")), None);
        assert_eq!(view.get(&key(first)), Some(&Type::atom("reinsert")));
        assert_eq!(view.get(&key(second)), Some(&Type::atom("collision")));
    }
    #[test]
    fn rest_domains_use_native_lookup_without_bank_and_preserve_retained_map_height_identity() {
        reset();
        let rest = vec![
            BindingKey::Label(VariableId::Integer((-1).into())),
            BindingKey::Label(VariableId::Atom("rest".into())),
            BindingKey::Label(VariableId::Integer("9223372036854775808".parse().unwrap())),
            BindingKey::RawVariableRecord(VariableId::Integer(1.into())),
        ];
        let initial: BindingMap = rest.iter().cloned().map(|key| (key, Type::any())).collect();
        let mut native = initial.clone();
        let mut view = ReadView::new(initial.clone());
        for k in &rest {
            assert_eq!(view.get(k), Some(&Type::any()));
            assert_eq!(view.get(k), Some(&Type::any()));
        }
        assert!(view.bank.get().is_none());
        assert_eq!(DIAGNOSTICS.get().raw_rest_reads, rest.len() * 2);
        assert!(view.native_map().ptr_eq(&initial));
        for k in rest.iter().cloned().chain([key(2_083_339)]) {
            assert_eq!(
                view.insert(k.clone(), Type::atom("changed")),
                native.insert(k.clone(), Type::atom("changed"))
            );
            assert_eq!(view.get(&k), native.get(&k));
            assert_eq!(view.remove(&k), native.remove(&k));
        }
        let output = view.into_native();
        assert_eq!(output, native);
        assert_eq!(output.labels.height, native.labels.height);
        assert!(!output.ptr_eq(&initial));
        assert_eq!(initial.get(&rest[0]), Some(&Type::any()));
    }
    #[test]
    fn cached_reads_keep_root_observation_and_native_write_touched_guards() {
        reset();
        let a = key(1);
        let b = key(other_slot(1));
        let input = BindingMap::from(vec![(a.clone(), Type::atom("input"))]);
        let mut catalogue = vec![a.clone(), b.clone()];
        catalogue.sort();
        let traced = input.trace_root(RootCatalogue::new(catalogue.into()));
        let mut view = ReadView::new(traced);
        assert_eq!(view.get(&a), Some(&Type::atom("input")));
        assert_eq!(view.get(&a), Some(&Type::atom("input")));
        assert_eq!(view.get(&b), None);
        assert_eq!(view.get(&b), None);
        view.insert(b.clone(), Type::any());
        assert_eq!(view.get(&b), Some(&Type::any()));
        let cert = view.into_native().certificate(&input);
        assert_eq!(cert.slots.len(), 2);
        assert!(cert.matches(&input));
        let mut changed = input.clone();
        changed.insert(b, Type::any());
        assert!(!cert.matches(&changed));
        assert_eq!(DIAGNOSTICS.get().logical_reads, 5);
    }
    #[test]
    fn borrowed_substitution_reaches_dynamic_map_entry_values_through_actual_atomic_core() {
        reset();
        let a = 1;
        let b = other_slot(a);
        let map = |value, default| {
            Type::map_types(
                vec![MapTypeEntry {
                    key: Type::atom("entry"),
                    presence: Presence::Mandatory,
                    value,
                }],
                Type::all_atoms(),
                default,
            )
            .unwrap()
        };
        let original = map(
            Type::variable_integer(a.into()),
            Type::variable_integer(b.into()),
        );
        assert!(
            !original
                .collect_var_names()
                .contains(&VariableId::Integer(a.into()))
        );
        let input = BindingMap::from(vec![
            (key(a), Type::atom("value")),
            (key(b), Type::atom("default")),
        ]);
        let mut view = ReadView::new(input.clone());
        let lookup = |id: &VariableId| view.get(&BindingKey::Label(id.clone()));
        assert_eq!(
            original.substitute_with(&lookup).unwrap(),
            map(Type::atom("value"), Type::atom("default"))
        );
        let atom = AtomicConstraint {
            left: original,
            relation: Relation::Subtype,
            right: ConstraintValue::Type(map(Type::atom("value"), Type::atom("default"))),
        };
        let status = atom
            .solve_access(&mut view, &DerivedArena::default(), &NoBifProvider, None)
            .unwrap();
        let AtomicStatus::Solved(updated) = status else {
            panic!("must solve")
        };
        assert_eq!(updated, KeySet::new());
        assert!(DIAGNOSTICS.get().positive_hits >= 2);
        assert!(view.into_native().ptr_eq(&input));
    }
    #[test]
    fn view_refinement_keeps_write_log_old_options_and_failed_sequence() {
        use crate::constraints::atomic::refine_values_access;
        use crate::lattice::UnificationValue;
        reset();
        let a = key(1);
        let b = key(other_slot(1));
        let initial = BindingMap::from(vec![(b.clone(), Type::any())]);
        let mut view = ReadView::new(initial.clone());
        assert_eq!(view.get(&a), None);
        assert_eq!(view.get(&b), Some(&Type::any()));
        let mut log = vec![];
        let status = refine_values_access(
            [
                (a.clone(), UnificationValue::Type(Type::atom("first"))),
                (b.clone(), UnificationValue::Type(Type::atom("narrowed"))),
                (b.clone(), UnificationValue::Type(Type::any())),
                (a.clone(), UnificationValue::Type(Type::atom("conflict"))),
                (key(999), UnificationValue::Type(Type::atom("unreached"))),
            ],
            &mut view,
            KeySet::new(),
            Some(&mut log),
        )
        .unwrap();
        assert!(matches!(status, AtomicStatus::Failed));
        assert_eq!(
            log,
            vec![
                (a.clone(), None),
                (b.clone(), Some(Type::any())),
                (b.clone(), Some(Type::atom("narrowed"))),
            ]
        );
        assert_eq!(view.get(&a), Some(&Type::atom("first")));
        assert_eq!(view.get(&b), None);
        assert_eq!(view.get(&key(999)), None);
        assert_eq!(initial.get(&a), None);
        assert_eq!(initial.get(&b), Some(&Type::any()));
    }
    #[test]
    fn verify_raw_hit_guard_rejects_stale_values_without_shadow() {
        if !crate::lattice::verify_fast_paths() {
            return;
        }
        reset();
        for positive in [true, false] {
            let mut view = ReadView::new(if positive {
                BindingMap::from(vec![(key(1), Type::atom("old"))])
            } else {
                BindingMap::new()
            });
            view.backing.shadow = None;
            let _ = view.get(&key(1));
            // Test-only fault injection bypasses the private write gate.
            view.backing.insert(key(1), Type::atom("changed"));
            assert!(
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let _ = view.get(&key(1));
                }))
                .is_err()
            );
        }
    }
    struct RefusalReset;
    impl Drop for RefusalReset {
        fn drop(&mut self) {
            REFUSAL.set(0);
        }
    }
    #[test]
    fn allocation_refusal_permanently_uses_current_native_operation_once() {
        for refusal in [1, 2] {
            reset();
            REFUSAL.set(refusal);
            let _reset = RefusalReset;
            let mut view = ReadView::new(BindingMap::new());
            assert_eq!(view.get(&key(1)), None);
            REFUSAL.set(0);
            assert_eq!(view.get(&key(1)), None);
            view.insert(key(1), Type::atom("written"));
            assert_eq!(view.get(&key(1)), Some(&Type::atom("written")));
            let d = DIAGNOSTICS.get();
            assert_eq!(
                (
                    d.logical_reads,
                    d.raw_trie_reads,
                    d.allocation_attempts,
                    d.allocation_refusals
                ),
                (3, 3, 1, 1)
            );
            assert_eq!(d.banks_initialized, 0);
            assert!(matches!(view.bank.get(), Some(BankState::Disabled)));
        }
    }
}
