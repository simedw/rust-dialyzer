use crate::native_runtime_hash::{
    HashState, NativeAtomLayout, NativeHashKey as Key, RuntimeHashError,
};
use num_bigint::Sign;
use num_traits::ToPrimitive;
use std::collections::BTreeMap;

enum Node<'a> {
    Atomic(&'a Key),
    Tuple(Vec<usize>),
    Cons(usize, usize),
    FlatMap(Vec<(usize, usize)>),
    Hamt {
        tag: u64,
        size: Option<usize>,
        children: Vec<usize>,
    },
}
struct Arena<'a> {
    nodes: Vec<Node<'a>>,
    originals: BTreeMap<usize, &'a Key>,
}
impl<'a> Arena<'a> {
    fn push(&mut self, node: Node<'a>) -> usize {
        let id = self.nodes.len();
        self.nodes.push(node);
        id
    }
    fn append(
        &mut self,
        key: &'a Key,
        layout: &NativeAtomLayout,
    ) -> Result<usize, RuntimeHashError> {
        let id = match key {
            Key::Tuple(xs) => {
                let children = xs
                    .iter()
                    .map(|x| self.append(x, layout))
                    .collect::<Result<Vec<_>, _>>()?;
                self.push(Node::Tuple(children))
            }
            Key::Cons(h, t) => {
                let h = self.append(h, layout)?;
                let t = self.append(t, layout)?;
                self.push(Node::Cons(h, t))
            }
            Key::Map(entries) => {
                let mut nodes = BTreeMap::new();
                for (key, value) in entries {
                    let k = self.append(key, layout)?;
                    let v = self.append(value, layout)?;
                    nodes.insert(key.clone(), (k, v));
                }
                let order = crate::native_map_order::ordered_keys(layout, nodes.keys().cloned())?;
                if nodes.len() <= 32 {
                    self.push(Node::FlatMap(order.iter().map(|key| nodes[key]).collect()))
                } else {
                    let rows = order
                        .iter()
                        .map(|key| Ok((layout.internal_hash(key)?, nodes[key])))
                        .collect::<Result<Vec<_>, RuntimeHashError>>()?;
                    self.hamt(rows, 0, true)?
                }
            }
            _ => self.push(Node::Atomic(key)),
        };
        self.originals.insert(id, key);
        Ok(id)
    }
    fn hamt(
        &mut self,
        rows: Vec<(u64, (usize, usize))>,
        depth: u32,
        root: bool,
    ) -> Result<usize, RuntimeHashError> {
        if depth == 16 {
            let mut rows = rows;
            rows.sort_by(|(_, (ak, _)), (_, (bk, _))| {
                crate::native_map_order::term_order(self.originals[ak], self.originals[bk])
            });
            let children = rows
                .into_iter()
                .map(|(_, (k, v))| self.push(Node::Cons(k, v)))
                .collect();
            return Ok(self.push(Node::Tuple(children)));
        }
        let total = rows.len();
        let mut groups = BTreeMap::<u64, Vec<_>>::new();
        for row in rows {
            groups
                .entry((row.0 >> (depth * 4)) & 15)
                .or_default()
                .push(row);
        }
        let tag = if root {
            if groups.len() == 16 { 8 } else { 9 }
        } else {
            10
        };
        let mut children = Vec::new();
        for rows in groups.into_values() {
            children.push(if rows.len() == 1 {
                let (_, (k, v)) = rows[0];
                self.push(Node::Cons(k, v))
            } else {
                self.hamt(rows, depth + 1, false)?
            });
        }
        Ok(self.push(Node::Hamt {
            tag,
            size: root.then_some(total),
            children,
        }))
    }
    fn immediate(
        &self,
        id: usize,
        layout: &NativeAtomLayout,
    ) -> Result<Option<u64>, RuntimeHashError> {
        match self.nodes[id] {
            Node::Atomic(k) => layout.immediate(k),
            _ => Ok(None),
        }
    }
}
enum Deferred {
    Key(usize),
    Cdr,
}
fn push_node(
    arena: &Arena<'_>,
    layout: &NativeAtomLayout,
    id: usize,
    state: &mut HashState,
    stack: &mut Vec<Deferred>,
) -> Result<(), RuntimeHashError> {
    if let Some(immediate) = arena.immediate(id, layout)? {
        state.alpha(1);
        state.beta(immediate);
    } else {
        state.alpha(2);
        stack.push(Deferred::Key(id));
    }
    Ok(())
}
pub(crate) fn hash(layout: &NativeAtomLayout, key: &Key) -> Result<u64, RuntimeHashError> {
    let mut arena = Arena {
        nodes: Vec::new(),
        originals: BTreeMap::new(),
    };
    let mut current = arena.append(key, layout)?;
    let mut state = HashState::default();
    let mut stack = Vec::new();
    loop {
        match &arena.nodes[current] {
            Node::Tuple(xs) => {
                state.alpha(6);
                state.beta(xs.len() as u64);
                if let Some((last, rest)) = xs.split_last() {
                    for id in rest {
                        push_node(&arena, layout, *id, &mut state, &mut stack)?;
                    }
                    current = *last;
                    continue;
                }
            }
            Node::FlatMap(entries) => {
                state.alpha(7);
                state.beta(entries.len() as u64);
                if let Some((last, rest)) = entries.split_last() {
                    for (k, v) in rest {
                        push_node(&arena, layout, *v, &mut state, &mut stack)?;
                        push_node(&arena, layout, *k, &mut state, &mut stack)?;
                    }
                    push_node(&arena, layout, last.1, &mut state, &mut stack)?;
                    current = last.0;
                    continue;
                }
            }
            Node::Hamt {
                tag,
                size,
                children,
            } => {
                state.alpha(*tag);
                if let Some(size) = size {
                    state.beta(*size as u64);
                }
                for id in children {
                    if let Node::Cons(k, v) = arena.nodes[*id] {
                        push_node(&arena, layout, v, &mut state, &mut stack)?;
                        push_node(&arena, layout, k, &mut state, &mut stack)?;
                    } else {
                        stack.push(Deferred::Key(*id));
                    }
                }
            }
            Node::Cons(_, _) => {
                let mut value = 0u64;
                let mut bytes = 0u64;
                while let Node::Cons(h, t) = arena.nodes[current] {
                    let Node::Atomic(Key::Integer(byte)) = arena.nodes[h] else {
                        break;
                    };
                    let Some(byte) = byte.to_u8() else { break };
                    value = (value << 8) | u64::from(byte);
                    bytes += 1;
                    if bytes == 4 {
                        state.alpha(5 | (bytes << 8) | (value << 32));
                        value = 0;
                        bytes = 0;
                    }
                    current = t;
                }
                if bytes > 0 {
                    state.alpha(5 | (bytes << 8) | (value << 32));
                }
                if let Node::Cons(h, t) = arena.nodes[current] {
                    if let Some(immediate) = arena.immediate(h, layout)? {
                        state.alpha(1 | (3 << 32));
                        state.beta(immediate);
                        if !matches!(arena.nodes[t], Node::Cons(_, _)) {
                            state.alpha(4);
                        }
                        current = t;
                    } else {
                        stack.push(Deferred::Key(t));
                        if !matches!(arena.nodes[t], Node::Cons(_, _)) {
                            stack.push(Deferred::Cdr);
                        }
                        state.alpha(3);
                        current = h;
                    }
                }
                continue;
            }
            Node::Atomic(k) => match k {
                Key::ExternalFun {
                    module,
                    function,
                    arity,
                } => {
                    return Err(RuntimeHashError::MissingExternalFunIdentity {
                        module: module.clone(),
                        function: function.clone(),
                        arity: *arity,
                    });
                }
                Key::Integer(value) => {
                    if let Some(immediate) = layout.immediate(k)? {
                        state.alpha(1);
                        state.beta(immediate);
                    } else {
                        let (sign, digits) = value.to_u64_digits();
                        state.alpha(
                            (if sign == Sign::Minus { 14 } else { 15 })
                                | ((digits.len() as u64) << 32),
                        );
                        let mut i = 0;
                        while i + 2 <= digits.len() {
                            state.alpha(digits[i]);
                            state.beta(digits[i + 1]);
                            i += 2;
                        }
                        if i < digits.len() {
                            state.beta(digits[i]);
                        }
                    }
                }
                Key::Float(bits) => {
                    state.alpha(20);
                    state.beta(*bits);
                }
                Key::Bits { bytes, bits } => {
                    state.alpha(11);
                    state.beta(*bits);
                    let count = (*bits / 8) as usize;
                    let mut i = 0;
                    while i + 16 <= count {
                        state.alpha(u64::from_le_bytes(
                            bytes[i..i + 8].try_into().expect("eight bytes"),
                        ));
                        state.beta(u64::from_le_bytes(
                            bytes[i + 8..i + 16].try_into().expect("eight bytes"),
                        ));
                        i += 16;
                    }
                    let remainder = &bytes[i..count];
                    if remainder.len() > 8 {
                        let mut tail = [0u8; 8];
                        tail[..remainder.len() - 8].copy_from_slice(&remainder[8..]);
                        state.beta_tail(u64::from_le_bytes(tail));
                    }
                    if !remainder.is_empty() {
                        let mut tail = [0u8; 8];
                        let n = remainder.len().min(8);
                        tail[..n].copy_from_slice(&remainder[..n]);
                        state.alpha_tail(u64::from_le_bytes(tail));
                    }
                    if bits % 8 != 0 {
                        state.alpha(u64::from(bytes[count] >> (8 - bits % 8)));
                    }
                }
                _ => {
                    state.alpha(1);
                    state.beta(layout.immediate(k)?.expect("atomic immediate"));
                }
            },
        }
        match stack.pop() {
            Some(Deferred::Key(id)) => current = id,
            Some(Deferred::Cdr) => {
                state.beta(4);
                let Some(Deferred::Key(id)) = stack.pop() else {
                    unreachable!("CDR follows tail")
                };
                current = id;
            }
            None => return Ok(state.finish()),
        }
    }
}
