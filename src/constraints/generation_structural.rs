use super::*;
impl Generator<'_> {
    pub(super) fn product_constraints(&mut self, args: Vec<Type>) -> Result<Type> {
        let mut unique = args.iter().map(Type::to_native_value).collect::<Vec<_>>();
        unique.sort();
        unique.dedup();
        if unique.len() == args.len() {
            return Ok(Type::product(args));
        }
        let vars = (0..args.len())
            .map(|_| self.fresh())
            .collect::<Result<Vec<_>>>()?;
        self.store_constraint_lists(
            vars.clone(),
            Relation::Equal,
            args.into_iter().map(NativeOperand::Type).collect(),
        )?;
        Ok(Type::product(vars))
    }
    pub(super) fn tuple_constraints(
        &mut self,
        args: Vec<Type>,
        foldable: bool,
        record: Option<Type>,
    ) -> Result<Type> {
        let mut es = Vec::new();
        for a in args {
            if !foldable && a.has_var() {
                let v = self.fresh()?;
                self.equal(a, v.clone())?;
                es.push(v);
            } else {
                es.push(a);
            }
        }
        let tuple = Type::tuple(es)?;
        if let Some(record) = record {
            self.atomic(
                tuple.clone(),
                Relation::Subtype,
                NativeOperand::Type(record),
            )?;
        }
        Ok(tuple)
    }
    pub(super) fn source_record(&mut self, site: &Site) -> Result<Option<Type>> {
        let CoreExpr::Tuple(elements) = &self.module(site).nodes[site.node.0].expr else {
            return Ok(None);
        };
        let Some(tag) = elements.first() else {
            return Ok(None);
        };
        let Some(ErlangTerm::Atom(tag)) = self.literal_term(site, *tag) else {
            return Ok(None);
        };
        let key = (site.module, tag.clone(), elements.len() - 1);
        Ok(if self.has_annotation(site, "record") {
            self.lookup_record(key.0, &key.1, key.2)?
        } else {
            None
        })
    }
    pub(super) fn cons_constraints(
        &mut self,
        value: Type,
        args: Vec<Type>,
        foldable: bool,
    ) -> Result<Type> {
        let [head, tail]: [Type; 2] = args
            .try_into()
            .map_err(|_| GenerationError::Boundary("cons results differ from two".into()))?;
        if foldable {
            return Ok(Type::cons(head, tail)?);
        }
        let cons = self.derived(
            DerivedOp::Cons {
                head: head.clone(),
                tail: tail.clone(),
            },
            "-traverse/3-fun-1-",
            vec![head.to_native_value(), tail.to_native_value()],
        )?;
        let hd = self.derived(
            DerivedOp::ConsHead {
                value: value.clone(),
            },
            "-traverse/3-fun-2-",
            vec![value.to_native_value()],
        )?;
        let tl = self.derived(
            DerivedOp::ConsTail {
                value: value.clone(),
            },
            "-traverse/3-fun-3-",
            vec![value.to_native_value()],
        )?;
        self.store_constraint_lists(
            vec![head, tail, value.clone()],
            Relation::Subtype,
            vec![hd, tl, cons],
        )?;
        Ok(value)
    }
}
