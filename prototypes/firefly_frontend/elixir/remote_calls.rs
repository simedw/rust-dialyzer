// SPDX-License-Identifier: Apache-2.0
use super::*;

#[derive(Clone, Copy)]
struct Reorder {
    module: &'static str,
    name: &'static str,
    indices: &'static [usize],
    increment_first: bool,
}

fn reorder(module: &str, name: &str, arity: usize) -> Option<Reorder> {
    let (module, name, indices, increment_first): (_, _, &[usize], _) = match (module, name, arity)
    {
        ("Elixir.Kernel", "elem", 2) => ("erlang", "element", &[1, 0], true),
        ("Elixir.Kernel", "put_elem", 3) => ("erlang", "setelement", &[1, 0, 2], true),
        ("Elixir.Kernel", "is_map_key", 2) => ("erlang", "is_map_key", &[1, 0], false),
        ("Elixir.Map", "delete", 2) => ("maps", "remove", &[1, 0], false),
        ("Elixir.Map", "fetch", 2) => ("maps", "find", &[1, 0], false),
        ("Elixir.Map", "fetch!", 2) => ("maps", "get", &[1, 0], false),
        ("Elixir.Map", "has_key?", 2) => ("maps", "is_key", &[1, 0], false),
        ("Elixir.Map", "put", 3) => ("maps", "put", &[1, 2, 0], false),
        ("Elixir.Map", "replace!", 3) => ("maps", "update", &[1, 2, 0], false),
        ("Elixir.Process", "group_leader", 2) => ("erlang", "group_leader", &[1, 0], false),
        ("Elixir.Tuple", "delete_at", 2) => ("erlang", "delete_element", &[1, 0], true),
        ("Elixir.Tuple", "duplicate", 2) => ("erlang", "make_tuple", &[1, 0], false),
        ("Elixir.Tuple", "insert_at", 3) => ("erlang", "insert_element", &[1, 0, 2], true),
        _ => return None,
    };
    Some(Reorder {
        module,
        name,
        indices,
        increment_first,
    })
}

impl Lower {
    pub(super) fn try_reordered_remote(
        &mut self,
        ann: Id,
        module: &str,
        name: &str,
        raw_args: Id,
        s: &mut Scope,
        context: Context,
    ) -> Result<Option<Id>> {
        if self.compiler == CompilerVersion::V1_19 {
            return Ok(None);
        }
        let raw = self.items(raw_args)?;
        let Some(rule) = reorder(module, name, raw.len()) else {
            return Ok(None);
        };
        let first_is_var = self.tup(raw[0]).is_ok_and(|p| {
            p.len() == 3
                && matches!(self.get(p[0]), T::Atom(_))
                && matches!(self.get(p[2]), T::Atom(_))
        });
        let temporary = if first_is_var {
            None
        } else {
            Some(self.fresh(ann, s))
        };
        let mut args = self.args(raw_args, ann, s, context)?;
        let setup = temporary.map(|var| {
            let setup = self.ast("match", ann, vec![var, args[0]]);
            args[0] = var;
            setup
        });
        let mut reordered: Vec<_> = rule.indices.iter().map(|i| args[*i]).collect();
        if rule.increment_first {
            let index = self.tup(reordered[0])?;
            reordered[0] = if self.name(index[0])? == "integer" {
                let T::Integer(value) = self.get(index[2]) else {
                    return Err("invalid integer index".into());
                };
                let incremented = self.add(T::Integer(increment_integer(value)));
                self.ast("integer", index[1], vec![incremented])
            } else {
                let one = self.int(1);
                let one = self.ast("integer", ann, vec![one]);
                self.op(ann, "+", vec![reordered[0], one])
            };
        }
        let call = self.remote(ann, rule.module, rule.name, reordered);
        Ok(Some(if let Some(setup) = setup {
            let expressions = self.list(vec![setup, call]);
            self.ast("block", ann, vec![expressions])
        } else {
            call
        }))
    }
}

fn increment_integer(value: &str) -> String {
    let negative = value.starts_with('-');
    let digits = value.trim_start_matches(['-', '+']);
    let mut digits = digits.as_bytes().to_vec();
    if negative {
        let mut borrow = true;
        for digit in digits.iter_mut().rev() {
            if !borrow {
                break;
            }
            if *digit == b'0' {
                *digit = b'9';
            } else {
                *digit -= 1;
                borrow = false;
            }
        }
        let digits = String::from_utf8(digits).unwrap();
        let digits = digits.trim_start_matches('0');
        if digits.is_empty() {
            "0".into()
        } else {
            format!("-{digits}")
        }
    } else {
        let mut carry = true;
        for digit in digits.iter_mut().rev() {
            if !carry {
                break;
            }
            if *digit == b'9' {
                *digit = b'0';
            } else {
                *digit += 1;
                carry = false;
            }
        }
        if carry {
            digits.insert(0, b'1');
        }
        String::from_utf8(digits).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compiler_19_leaves_expanded_remote_calls_unchanged() {
        let mut lower = Lower {
            a: ErlangTerms {
                root: Id(0),
                terms: vec![T::Nil],
            },
            compiler: CompilerVersion::V1_19,
        };
        let ann = lower.int(0);
        let raw_args = lower.list(vec![]);
        assert!(
            lower
                .try_reordered_remote(
                    ann,
                    "Elixir.Map",
                    "put",
                    raw_args,
                    &mut Scope::default(),
                    Context::Expr
                )
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn index_increment_preserves_arbitrary_precision() {
        for (value, expected) in [
            ("0", "1"),
            ("-1", "0"),
            ("-10", "-9"),
            (
                "999999999999999999999999999999",
                "1000000000000000000000000000000",
            ),
            (
                "-1000000000000000000000000000000",
                "-999999999999999999999999999999",
            ),
        ] {
            assert_eq!(increment_integer(value), expected);
        }
    }
    #[test]
    fn only_compiler_reorders_are_registered() {
        assert_eq!(reorder("Elixir.Map", "put", 3).unwrap().indices, &[1, 2, 0]);
        assert!(reorder("Elixir.Map", "put", 2).is_none());
        assert!(reorder("Elixir.Map", "get", 2).is_none());
        assert!(reorder("Elixir.Kernel", "elem", 2).unwrap().increment_first);
    }
}
