mod arithmetic;
pub mod boundary;
mod callbacks;
mod list_special;
mod maps;
mod predicates;
pub mod registry;
mod structural;

pub use predicates::type_ranks;

#[cfg(test)]
mod transfer_checks {
    use crate::{
        bifs::registry::{Mfa, Registry, TransferError},
        lattice::Type,
    };
    fn registry() -> Registry {
        Registry::standard().unwrap()
    }
    fn mfa(name: &str, arity: usize) -> Mfa {
        Mfa {
            module: "erlang".into(),
            function: name.into(),
            arity,
        }
    }
    #[test]
    fn runtime_generic_is_strict_and_missing_summary_is_explicit() {
        let r = registry();
        for arity in [1, 2] {
            for (arg, result) in [
                (Type::any(), Type::any()),
                (Type::none(), Type::none()),
                (Type::unit(), Type::none()),
            ] {
                let out = r
                    .transfer(&mfa("get_module_info", arity), &vec![arg; arity])
                    .unwrap();
                assert!(!out.known);
                assert!(out.runtime_builtin);
                assert_eq!(out.result, result);
            }
        }
        assert!(matches!(
            r.transfer(&mfa("not_an_export", 1), &[Type::any()]),
            Err(TransferError::Unsupported { .. })
        ));
    }
    #[test]
    fn native_empty_record_tuple_is_an_error() {
        assert!(matches!(
            registry().transfer(
                &mfa("is_record", 2),
                &[Type::tuple(vec![]).unwrap(), Type::all_atoms()]
            ),
            Err(TransferError::NativeException(
                crate::native_error::NativeTransferError::CaseClauseNil
            ))
        ));
    }
    #[test]
    fn ordering_guard_branches_preserve_finite_sets() {
        let r = registry();
        let a = Type::integers([1.into(), 3.into(), 7.into()]);
        let b = Type::integer(3.into());
        assert_eq!(
            r.guard_refinements(&mfa("<", 2), &[a.clone(), b.clone()], true)
                .unwrap()
                .unwrap(),
            vec![Type::integer(1.into()), b.clone()]
        );
        assert_eq!(
            r.guard_refinements(&mfa("<", 2), &[a, b.clone()], false)
                .unwrap()
                .unwrap(),
            vec![Type::integers([3.into(), 7.into()]), b]
        );
    }
}
