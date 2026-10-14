use crate::{
    native_ets_set::NativeEtsSet,
    native_runtime_hash::{NativeAtomLayout, NativeHashKey, RuntimeHashError},
};
use std::collections::{BTreeMap, BTreeSet};

pub trait RuntimeGraphKey: Clone + Ord {
    fn runtime_key(&self) -> Result<NativeHashKey, RuntimeHashError>;
}
impl RuntimeGraphKey for NativeHashKey {
    fn runtime_key(&self) -> Result<NativeHashKey, RuntimeHashError> {
        Ok(self.clone())
    }
}
#[derive(Clone, Debug, Default)]
struct Direction<V> {
    edges: Vec<NativeHashKey>,
    /// In edge order, oldest first; `Neighbours` presents them newest first,
    /// as digraph does. Appending keeps adding an edge O(1) for vertices
    /// with thousands of callers.
    neighbours: Vec<V>,
}
/// A vertex's neighbours, newest edge first.
#[derive(Debug)]
pub struct Neighbours<'a, V>(&'a [V]);
impl<V> Clone for Neighbours<'_, V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V> Copy for Neighbours<'_, V> {}
impl<V> Default for Neighbours<'_, V> {
    fn default() -> Self {
        Self(&[])
    }
}
impl<'a, V> Neighbours<'a, V> {
    pub fn iter(&self) -> std::iter::Rev<std::slice::Iter<'a, V>> {
        self.0.iter().rev()
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    /// The neighbours in an unspecified order, for order-free inspection.
    pub fn as_unordered_slice(&self) -> &'a [V] {
        self.0
    }
}
impl<'a, V> IntoIterator for Neighbours<'a, V> {
    type Item = &'a V;
    type IntoIter = std::iter::Rev<std::slice::Iter<'a, V>>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}
#[derive(Clone, Debug)]
struct Vertex<V> {
    confirmed: bool,
    incoming: Direction<V>,
    outgoing: Direction<V>,
}
pub fn prehash_vertex<V: RuntimeGraphKey>(
    layout: Option<&NativeAtomLayout>,
    v: &V,
) -> Result<Option<(NativeHashKey, u64)>, RuntimeHashError> {
    let Some(layout) = layout else { return Ok(None) };
    let key = v.runtime_key()?;
    let hash = layout.ets_hash(&key)?;
    Ok(Some((key, hash)))
}
#[derive(Clone, Debug)]
struct Physical<V> {
    layout: NativeAtomLayout,
    vertices: NativeEtsSet<V>,
    edges: NativeEtsSet<()>,
}
/// One graph owns adjacency; ETS tables retain only physical enumeration history.
#[derive(Clone, Debug)]
pub struct SourceDigraph<V: RuntimeGraphKey> {
    vertices: BTreeMap<V, Vertex<V>>,
    edges: BTreeMap<NativeHashKey, (V, V)>,
    physical: Option<Physical<V>>,
    acyclic: bool,
    next_edge: i64,
}
impl<V: RuntimeGraphKey> SourceDigraph<V> {
    pub fn new(layout: Option<NativeAtomLayout>, acyclic: bool) -> Self {
        Self {
            vertices: BTreeMap::new(),
            edges: BTreeMap::new(),
            physical: layout.map(|layout| Physical {
                layout,
                vertices: NativeEtsSet::default(),
                edges: NativeEtsSet::default(),
            }),
            acyclic,
            next_edge: 0,
        }
    }
    pub fn add_vertex(&mut self, v: V, confirmed: bool) -> Result<(), RuntimeHashError> {
        let key = self.prehash_vertex(&v)?;
        self.add_vertex_hashed(v, key, confirmed);
        Ok(())
    }
    /// The physical table key and hash of `v`, if the graph has a layout.
    /// Pure, so callers may compute these ahead on other threads.
    pub fn prehash_vertex(&self, v: &V) -> Result<Option<(NativeHashKey, u64)>, RuntimeHashError> {
        prehash_vertex(self.physical.as_ref().map(|p| &p.layout), v)
    }
    /// `add_vertex` with `key` from `prehash_vertex`.
    pub fn add_vertex_hashed(&mut self, v: V, key: Option<(NativeHashKey, u64)>, confirmed: bool) {
        if let (Some(p), Some((key, hash))) = (&mut self.physical, key) {
            p.vertices.insert_hashed(key, hash, v.clone());
        }
        self.vertices
            .entry(v)
            .and_modify(|row| row.confirmed = confirmed)
            .or_insert_with(|| Vertex {
                confirmed,
                incoming: Direction {
                    edges: Vec::new(),
                    neighbours: Vec::new(),
                },
                outgoing: Direction {
                    edges: Vec::new(),
                    neighbours: Vec::new(),
                },
            });
    }
    pub fn contains(&self, v: &V) -> bool {
        self.vertices.contains_key(v)
    }
    pub fn is_confirmed(&self, v: &V) -> bool {
        self.vertices.get(v).is_some_and(|r| r.confirmed)
    }
    pub fn add_edge(
        &mut self,
        id: NativeHashKey,
        from: V,
        to: V,
    ) -> Result<bool, RuntimeHashError> {
        let hash = match &self.physical {
            Some(p) => Some(p.layout.ets_hash(&id)?),
            None => None,
        };
        self.add_edge_hashed(id, hash, from, to)
    }
    /// `add_edge` with the physical hash of `id` computed ahead.
    pub fn add_edge_hashed(
        &mut self,
        id: NativeHashKey,
        hash: Option<u64>,
        from: V,
        to: V,
    ) -> Result<bool, RuntimeHashError> {
        if !self.contains(&from) || !self.contains(&to) {
            return Err(RuntimeHashError::Boundary("digraph bad_vertex".into()));
        }
        if let Some((old_from, old_to)) = self.edges.get(&id) {
            if old_from != &from || old_to != &to {
                return Err(RuntimeHashError::Boundary(
                    "digraph edge identity changed endpoints".into(),
                ));
            }
            return Ok(true);
        }
        if self.acyclic && (from == to || self.has_path(&to, &from)) {
            return Ok(false);
        }
        if let (Some(p), Some(hash)) = (&mut self.physical, hash) {
            p.edges.insert_hashed(id.clone(), hash, ());
        }
        if let Some(row) = self.vertices.get_mut(&from) {
            row.outgoing.edges.push(id.clone());
            row.outgoing.neighbours.push(to.clone());
        }
        if let Some(row) = self.vertices.get_mut(&to) {
            row.incoming.edges.push(id.clone());
            row.incoming.neighbours.push(from.clone());
        }
        self.edges.insert(id, (from, to));
        Ok(true)
    }
    pub fn auto_edge(&mut self, from: V, to: V) -> Result<bool, RuntimeHashError> {
        let id = NativeHashKey::Cons(
            Box::new(NativeHashKey::Atom("$e".into())),
            Box::new(NativeHashKey::Integer(self.next_edge.into())),
        );
        self.next_edge = self
            .next_edge
            .checked_add(1)
            .ok_or_else(|| RuntimeHashError::Boundary("digraph edge counter exhausted".into()))?;
        self.add_edge(id, from, to)
    }
    /// `auto_edge` for a graph that is only traversed: adjacency is recorded,
    /// but the edge is not entered in the edge index or the physical edge
    /// table, so `edge`, `ordered_edges` and `unordered_edges` omit it.
    pub fn auto_edge_adjacency(&mut self, from: V, to: V) -> Result<(), RuntimeHashError> {
        if !self.contains(&from) || !self.contains(&to) || self.acyclic {
            return self.auto_edge(from, to).map(drop);
        }
        let id = NativeHashKey::Cons(
            Box::new(NativeHashKey::Atom("$e".into())),
            Box::new(NativeHashKey::Integer(self.next_edge.into())),
        );
        self.next_edge = self
            .next_edge
            .checked_add(1)
            .ok_or_else(|| RuntimeHashError::Boundary("digraph edge counter exhausted".into()))?;
        if let Some(row) = self.vertices.get_mut(&from) {
            row.outgoing.edges.push(id.clone());
            row.outgoing.neighbours.push(to.clone());
        }
        if let Some(row) = self.vertices.get_mut(&to) {
            row.incoming.edges.push(id);
            row.incoming.neighbours.push(from);
        }
        Ok(())
    }
    pub fn incoming(&self, v: &V) -> Option<Neighbours<'_, V>> {
        self.vertices
            .get(v)
            .map(|r| Neighbours(r.incoming.neighbours.as_slice()))
            .filter(|r| !r.is_empty())
    }
    pub fn outgoing(&self, v: &V) -> Option<Neighbours<'_, V>> {
        self.vertices
            .get(v)
            .map(|r| Neighbours(r.outgoing.neighbours.as_slice()))
            .filter(|r| !r.is_empty())
    }
    pub fn in_edges(&self, v: &V) -> &[NativeHashKey] {
        self.vertices
            .get(v)
            .map_or(&[], |r| r.incoming.edges.as_slice())
    }
    pub fn out_edges(&self, v: &V) -> &[NativeHashKey] {
        self.vertices
            .get(v)
            .map_or(&[], |r| r.outgoing.edges.as_slice())
    }
    pub fn edge(&self, id: &NativeHashKey) -> Option<&(V, V)> {
        self.edges.get(id)
    }
    /// Every edge's endpoints, in no runtime order: for callers whose
    /// result does not depend on the order (sets and maps of endpoints).
    pub fn unordered_edges(&self) -> impl Iterator<Item = &(V, V)> {
        self.edges.values()
    }
    /// Every vertex, in no runtime order (see `unordered_edges`).
    pub fn unordered_vertices(&self) -> impl Iterator<Item = &V> {
        self.vertices.keys()
    }
    pub fn ordered_vertices(&self) -> Result<Vec<V>, RuntimeHashError> {
        let p = self
            .physical
            .as_ref()
            .ok_or(RuntimeHashError::UnavailableAtomLayout)?;
        // Each ETS row holds its vertex (runtime keys identify vertices), so
        // no key-to-vertex index is built per enumeration.
        let ordered = p
            .vertices
            .rows()
            .into_iter()
            .map(|(_, v)| v.clone())
            .collect::<Vec<_>>();
        if crate::lattice::verify_fast_paths() {
            let lookup = self
                .vertices
                .keys()
                .map(|v| Ok((v.runtime_key()?, v)))
                .collect::<Result<BTreeMap<_, _>, RuntimeHashError>>()?;
            let indexed = p
                .vertices
                .select_keys()
                .iter()
                .map(|key| lookup.get(key).map(|v| (*v).clone()))
                .collect::<Option<Vec<_>>>();
            assert!(indexed.as_ref() == Some(&ordered), "vertex ETS rows differ from the vertex index");
        }
        Ok(ordered)
    }
    pub fn ordered_edges(&self) -> Result<Vec<NativeHashKey>, RuntimeHashError> {
        Ok(self
            .physical
            .as_ref()
            .ok_or(RuntimeHashError::UnavailableAtomLayout)?
            .edges
            .select_keys())
    }
    pub fn delete_vertex(&mut self, v: &V) -> Result<(), RuntimeHashError> {
        let incident = self
            .in_edges(v)
            .iter()
            .chain(self.out_edges(v))
            .cloned()
            .collect::<Vec<_>>();
        for edge in incident {
            self.delete_edge(&edge)?;
        }
        if let Some(p) = &mut self.physical {
            p.vertices.remove(&v.runtime_key()?, &p.layout)?;
        }
        self.vertices.remove(v);
        Ok(())
    }
    pub fn delete_edge(&mut self, id: &NativeHashKey) -> Result<(), RuntimeHashError> {
        let Some((from, to)) = self.edges.remove(id) else {
            return Ok(());
        };
        if let Some(row) = self.vertices.get_mut(&from) {
            remove_direction(&mut row.outgoing, id);
        }
        if let Some(row) = self.vertices.get_mut(&to) {
            remove_direction(&mut row.incoming, id);
        }
        if let Some(p) = &mut self.physical {
            p.edges.remove(id, &p.layout)?;
        }
        Ok(())
    }
    pub fn layout(&self) -> Result<&NativeAtomLayout, RuntimeHashError> {
        self.physical
            .as_ref()
            .map(|p| &p.layout)
            .ok_or(RuntimeHashError::UnavailableAtomLayout)
    }
    fn has_path(&self, from: &V, to: &V) -> bool {
        let mut pending = vec![from.clone()];
        let mut seen = BTreeSet::new();
        while let Some(v) = pending.pop() {
            if &v == to {
                return true;
            }
            if seen.insert(v.clone()) {
                pending.extend(self.outgoing(&v).unwrap_or_default().iter().cloned());
            }
        }
        false
    }
}
fn remove_direction<V>(direction: &mut Direction<V>, edge: &NativeHashKey) {
    if let Some(position) = direction.edges.iter().position(|id| id == edge) {
        direction.neighbours.remove(position);
        direction.edges.remove(position);
    }
}
