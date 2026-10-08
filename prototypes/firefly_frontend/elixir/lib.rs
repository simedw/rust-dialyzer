// SPDX-License-Identifier: Apache-2.0
// Translation semantics derived from Elixir compiler sources.
// Copyright 2021 The Elixir Team; Copyright 2012 Plataformatec.
use rust_dialyzer_poc::core::{ErlangTerm as T, ErlangTerms, TermId as Id};
use std::collections::BTreeMap;
mod arena;
mod comprehensions;
mod control;
mod exceptions;
mod expressions;
mod remote_calls;
mod forms;
mod literals;
mod md5;
use arena::validate_input;
pub use forms::{translate, translate_with_version};
use md5::md5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompilerVersion {
    V1_19,
    V1_20,
}
impl CompilerVersion {
    pub fn parse(version: &str) -> std::result::Result<Self, String> {
        let parts = version.split('.').collect::<Vec<_>>();
        match parts.as_slice() {
            ["1", "19", patch] if patch.parse::<u32>().is_ok() => Ok(Self::V1_19),
            ["1", "20", patch] if patch.parse::<u32>().is_ok() => Ok(Self::V1_20),
            _ => Err(format!("unsupported Elixir compiler version {version}")),
        }
    }
    fn variable_name(self, count: usize) -> String {
        match self {
            Self::V1_19 => format!("_@{count}"),
            Self::V1_20 => format!("_{count}"),
        }
    }
}

type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    Expr,
    Match,
    Guard,
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum PatternBinding {
    #[default]
    Ordinary,
    FreshParameter,
}
#[derive(Default, Clone)]
struct Scope {
    stacktrace: Option<String>,
    caller: bool,
    versions: BTreeMap<String, String>,
    counters: BTreeMap<String, usize>,
    extra_guards: Vec<Id>,
    pattern_binding: PatternBinding,
}
struct Lower {
    a: ErlangTerms,
    compiler: CompilerVersion,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn md5_known_vectors() {
        assert_eq!(
            md5(b""),
            [
                0xd4, 0x1d, 0x8c, 0xd9, 0x8f, 0x00, 0xb2, 0x04, 0xe9, 0x80, 0x09, 0x98, 0xec, 0xf8,
                0x42, 0x7e
            ]
        );
        assert_eq!(
            md5(b"abc"),
            [
                0x90, 0x01, 0x50, 0x98, 0x3c, 0xd2, 0x4f, 0xb0, 0xd6, 0x96, 0x3f, 0x7d, 0x28, 0xe1,
                0x7f, 0x72
            ]
        );
    }
}

#[cfg(test)]
mod boundary_tests {
    use super::*;
    fn lower() -> Lower {
        Lower {
            compiler: CompilerVersion::V1_20,
            a: ErlangTerms {
                root: Id(0),
                terms: vec![T::Nil],
            },
        }
    }
    #[test]
    fn malformed_arena_is_rejected_before_traversal() {
        let a = ErlangTerms {
            root: Id(0),
            terms: vec![T::Tuple(vec![Id(0)])],
        };
        assert!(translate(&a, Id(0)).unwrap_err().contains("acyclic"));
    }
    #[test]
    fn deeply_nested_metadata_is_bounded() {
        let mut a = ErlangTerms {
            root: Id(0),
            terms: vec![T::Nil],
        };
        for _ in 0..257 {
            let next = Id(a.terms.len());
            a.terms.push(T::Tuple(vec![a.root]));
            a.root = next;
        }
        assert!(translate(&a, a.root).unwrap_err().contains("nesting"));
    }
    #[test]
    fn version_identity_overrides_source_name_and_context() {
        let mut l = lower();
        let zero = l.int(0);
        let version = l.atom("version");
        let pair = l.tuple(vec![version, zero]);
        let meta = l.list(vec![pair]);
        let first = l.atom("first");
        let kind = l.atom("Kernel");
        let x = l.tuple(vec![first, meta, kind]);
        let second = l.atom("second");
        let kind = l.atom("nil");
        let y = l.tuple(vec![second, meta, kind]);
        let mut s = Scope::default();
        let x = l.expr(x, zero, &mut s, Context::Expr, 0).unwrap();
        let y = l.expr(y, zero, &mut s, Context::Expr, 0).unwrap();
        let x = l.tup(x).unwrap();
        let y = l.tup(y).unwrap();
        assert_eq!(l.name(x[2]).unwrap(), "_1");
        assert_eq!(l.name(y[2]).unwrap(), "_1");
    }
    #[test]
    fn metadata_generation_keeps_exact_column() {
        let mut l = lower();
        let line = l.int(12);
        let column = l.int(9);
        let linekey = l.atom("line");
        let colkey = l.atom("column");
        let genkey = l.atom("generated");
        let yes = l.atom("true");
        let p = l.tuple(vec![linekey, line]);
        let q = l.tuple(vec![colkey, column]);
        let g = l.tuple(vec![genkey, yes]);
        let meta = l.list(vec![p, q, g]);
        let ann = l.ann(meta).unwrap();
        let location = l.prop(ann, "location").unwrap().unwrap();
        let loc = l.tup(location).unwrap();
        assert_eq!(l.integer(loc[0]).unwrap(), 12);
        assert_eq!(l.integer(loc[1]).unwrap(), 9);
    }
    #[test]
    fn generated_annotations_are_idempotent() {
        let mut l = lower();
        let line = l.int(42);
        let first = l.generated(line);
        let second = l.generated(first);
        assert_eq!(first, second);
        let loc = l.prop(second, "location").unwrap().unwrap();
        assert_eq!(l.integer(loc).unwrap(), 42);
    }
    #[test]
    fn compiler_profile_controls_generated_variable_identity() {
        assert_eq!(CompilerVersion::parse("1.19.4").unwrap().variable_name(7), "_@7");
        assert_eq!(CompilerVersion::parse("1.20.4").unwrap().variable_name(7), "_7");
        assert!(CompilerVersion::parse("1.21.0").is_err());
        assert!(CompilerVersion::parse("unknown").is_err());
    }
    #[test]
    fn dynamic_struct_pattern_requires_atom_module_and_consumes_guards() {
        let mut l = lower();
        let ann = l.int(7);
        let version_key = l.atom("version");
        let version = l.int(0);
        let version = l.tuple(vec![version_key, version]);
        let meta = l.list(vec![version]);
        let name = l.atom("module");
        let nil = l.atom("nil");
        let module = l.tuple(vec![name, meta, nil]);
        let empty = l.list(vec![]);
        let map = l.atom("%{}");
        let map = l.tuple(vec![map, empty, empty]);
        let mut scope = Scope::default();
        let result = l.structure(&[module, map], ann, &mut scope, Context::Match).unwrap();
        let fields = l.tup(result).unwrap();
        let fields = l.items(fields[2]).unwrap();
        let field = l.tup(fields[0]).unwrap();
        let matched = l.tup(field[3]).unwrap();
        assert_eq!(l.name(matched[0]).unwrap(), "match");
        assert_eq!(scope.extra_guards.len(), 1);
        let guards = l.take_pattern_guards(&mut scope, vec![], ann).unwrap();
        assert!(scope.extra_guards.is_empty());
        assert_eq!(guards.len(), 1);
        let group = l.items(guards[0]).unwrap();
        let call = l.tup(group[0]).unwrap();
        let remote = l.tup(call[2]).unwrap();
        let name = l.tup(remote[3]).unwrap();
        assert_eq!(l.name(name[2]).unwrap(), "is_atom");
        let arguments = l.items(call[3]).unwrap();
        assert_eq!(arguments[0], matched[2]);
    }
    #[test]
    fn expanded_super_calls_metadata_target_and_threads_macro_caller() {
        for kind in ["def", "defp", "defmacro", "defmacrop"] {
            let mut l = lower();
            let ann = l.int(7);
            let target = l.atom("original");
            let kind_id = l.atom(kind);
            let identity = l.tuple(vec![kind_id, target]);
            let key = l.atom("super");
            let prop = l.tuple(vec![key, identity]);
            let meta = l.list(vec![prop]);
            let arg = l.int(42);
            let args = l.list(vec![arg]);
            let expr = l.tuple(vec![key, meta, args]);
            let mut scope = Scope::default();
            let result = l.expr(expr, ann, &mut scope, Context::Expr, 0).unwrap();
            let call = l.tup(result).unwrap();
            assert_eq!(l.name(call[0]).unwrap(), "call");
            let target = l.tup(call[2]).unwrap();
            let macro_call = kind.starts_with("defmacro");
            assert_eq!(l.name(target[2]).unwrap(), if macro_call { "MACRO-original" } else { "original" });
            let args = l.items(call[3]).unwrap();
            assert_eq!(args.len(), if macro_call { 2 } else { 1 });
            assert_eq!(scope.caller, macro_call);
            if macro_call {
                let caller = l.tup(args[0]).unwrap();
                assert_eq!(l.name(caller[0]).unwrap(), "var");
                assert_eq!(l.name(caller[2]).unwrap(), "__CALLER__");
            }
        }
    }
    #[test]
    fn signed_zero_literal_uses_explicit_operator() {
        let mut l = lower();
        let ann = l.int(0);
        for (bits, sign) in [(0, "+"), (1u64 << 63, "-")] {
            let raw = l.add(T::Float(bits));
            let ast = l.lit(raw, ann).unwrap();
            let fields = l.tup(ast).unwrap();
            assert_eq!(l.name(fields[0]).unwrap(), "op");
            assert_eq!(l.name(fields[2]).unwrap(), sign);
        }
    }
}
