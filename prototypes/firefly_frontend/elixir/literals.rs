// SPDX-License-Identifier: Apache-2.0
use super::*;
impl Lower {
    pub(super) fn lit(&mut self, id: Id, ann: Id) -> Result<Id> {
        let raw = self.get(id).clone();
        Ok(match raw {
            T::Atom(_) => self.ast("atom", ann, vec![id]),
            T::ExternalFun { module, function, arity } => {
                let module = self.atom(&module);
                let module = self.ast("atom", ann, vec![module]);
                let function = self.atom(&function);
                let function = self.ast("atom", ann, vec![function]);
                let arity = self.int(i64::from(arity));
                let arity = self.ast("integer", ann, vec![arity]);
                let tag = self.atom("function");
                let capture = self.tuple(vec![tag, module, function, arity]);
                self.ast("fun", ann, vec![capture])
            }
            T::Integer(_) => self.ast("integer", ann, vec![id]),
            T::Float(bits) => {
                if f64::from_bits(bits) == 0.0 {
                    let zero = self.add(T::Float(0));
                    let value = self.ast("float", ann, vec![zero]);
                    self.op(ann, if bits >> 63 == 1 { "-" } else { "+" }, vec![value])
                } else {
                    self.ast("float", ann, vec![id])
                }
            }
            T::Nil => self.ast("nil", ann, vec![]),
            T::Tuple(v) => {
                let mut xs = vec![];
                for x in v {
                    xs.push(self.lit(x, ann)?)
                }
                let l = self.list(xs);
                self.ast("tuple", ann, vec![l])
            }
            T::List { items, tail } => {
                let mut t = self.lit(tail, ann)?;
                for x in items.into_iter().rev() {
                    let h = self.lit(x, ann)?;
                    t = self.ast("cons", ann, vec![h, t])
                }
                t
            }
            T::Binary(bytes) => {
                if bytes.is_empty() {
                    let empty = self.list(vec![]);
                    return Ok(self.ast("bin", ann, vec![empty]));
                }
                let mut xs = vec![];
                for b in bytes {
                    xs.push(self.int(b as i64))
                }
                let chars = self.list(xs);
                let string = self.ast("string", ann, vec![chars]);
                let default = self.atom("default");
                let seg = self.ast("bin_element", ann, vec![string, default, default]);
                let segs = self.list(vec![seg]);
                self.ast("bin", ann, vec![segs])
            }
            T::Map(pairs) => {
                let mut fs = vec![];
                for (k, v) in pairs {
                    let k = self.lit(k, ann)?;
                    let v = self.lit(v, ann)?;
                    fs.push(self.ast("map_field_assoc", ann, vec![k, v]))
                }
                let fs = self.list(fs);
                self.ast("map", ann, vec![fs])
            }
            T::Bitstring { .. } => return Err("unsupported non-byte-aligned literal".into()),
        })
    }
    pub(super) fn basic(&self, id: Id) -> bool {
        match self.get(id) {
            T::Atom(_) | T::Integer(_) | T::Float(_) | T::Binary(_) | T::Nil => true,
            T::List { items, tail } => items.iter().all(|x| self.basic(*x)) && self.basic(*tail),
            T::Tuple(p) if p.len() == 2 => p.iter().all(|x| self.basic(*x)),
            _ => false,
        }
    }
    pub(super) fn mapset_constant(&mut self, args: Id) -> Result<Option<Id>> {
        let args = self.items(args)?;
        let mut values = match args.as_slice() {
            [] => vec![],
            [arg] if self.basic(*arg) => {
                let Ok(values) = self.items(*arg) else {
                    return Ok(None);
                };
                values
            }
            _ => return Ok(None),
        };
        values.sort_by(|a, b| {
            rust_dialyzer_poc::term_order::compare_map_keys(&self.a, *a, &self.a, *b).unwrap()
        });
        values.dedup_by(|a, b| {
            rust_dialyzer_poc::term_order::compare_map_keys(&self.a, *a, &self.a, *b)
                .is_ok_and(|o| o.is_eq())
        });
        let empty = self.list(vec![]);
        let entries = values.into_iter().map(|x| (x, empty)).collect();
        let set = self.add(T::Map(entries));
        let structure = self.atom("__struct__");
        let mapset = self.atom("Elixir.MapSet");
        let map = self.atom("map");
        Ok(Some(
            self.add(T::Map(vec![(structure, mapset), (map, set)])),
        ))
    }
}
