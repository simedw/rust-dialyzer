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
    neighbours: Vec<V>,
}
#[derive(Clone, Debug)]
struct Vertex<V> {
    confirmed: bool,
    incoming: Direction<V>,
    outgoing: Direction<V>,
}
#[derive(Clone, Debug)]
struct Physical {
    layout: NativeAtomLayout,
    vertices: NativeEtsSet<()>,
    edges: NativeEtsSet<()>,
}
/// One graph owns adjacency; ETS tables retain only physical enumeration history.
#[derive(Clone, Debug)]
pub struct SourceDigraph<V: RuntimeGraphKey> {
    vertices: BTreeMap<V, Vertex<V>>,
    edges: BTreeMap<NativeHashKey, (V, V)>,
    physical: Option<Physical>,
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
        if let Some(p) = &mut self.physical {
            p.vertices.insert(v.runtime_key()?, (), &p.layout)?;
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
        Ok(())
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
        if let Some(p) = &mut self.physical {
            p.edges.insert(id.clone(), (), &p.layout)?;
        }
        if let Some(row) = self.vertices.get_mut(&from) {
            row.outgoing.edges.push(id.clone());
            row.outgoing.neighbours.insert(0, to.clone());
        }
        if let Some(row) = self.vertices.get_mut(&to) {
            row.incoming.edges.push(id.clone());
            row.incoming.neighbours.insert(0, from.clone());
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
    pub fn incoming(&self, v: &V) -> Option<&[V]> {
        self.vertices
            .get(v)
            .map(|r| r.incoming.neighbours.as_slice())
            .filter(|r| !r.is_empty())
    }
    pub fn outgoing(&self, v: &V) -> Option<&[V]> {
        self.vertices
            .get(v)
            .map(|r| r.outgoing.neighbours.as_slice())
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
    pub fn ordered_vertices(&self) -> Result<Vec<V>, RuntimeHashError> {
        let p = self
            .physical
            .as_ref()
            .ok_or(RuntimeHashError::UnavailableAtomLayout)?;
        let lookup = self
            .vertices
            .keys()
            .map(|v| Ok((v.runtime_key()?, v)))
            .collect::<Result<BTreeMap<_, _>, RuntimeHashError>>()?;
        p.vertices
            .select_keys()
            .iter()
            .map(|key| {
                lookup.get(key).map(|v| (*v).clone()).ok_or_else(|| {
                    RuntimeHashError::Boundary("vertex ETS history inconsistent".into())
                })
            })
            .collect()
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
                pending.extend(self.outgoing(&v).unwrap_or(&[]).iter().cloned());
            }
        }
        false
    }
}
fn remove_direction<V>(direction: &mut Direction<V>, edge: &NativeHashKey) {
    if let Some(position) = direction.edges.iter().position(|id| id == edge) {
        direction
            .neighbours
            .remove(direction.edges.len() - position - 1);
        direction.edges.remove(position);
    }
}
