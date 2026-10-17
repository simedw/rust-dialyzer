//! OTP29 erl_expand_records native records. Tuple-record expansion stays in its
//! existing implementation; these clauses precede it exactly as upstream does.
use super::*;
#[derive(Clone)]
pub(super) enum Definition { Local(Vec<TermId>), Imported(String) }
impl Expander {
    pub(super) fn init_native_records(&mut self, input: &[TermId]) -> Result<(), String> {
        for &id in input {
            let f = self.a.tuple_fields(id)?.to_vec();
            if self.a.tag(id) != Some("attribute") || f.len() != 4 { continue; }
            match self.a.as_atom(f[2]) {
                Some("module") => self.module = self.atom_name(f[3])?,
                Some("native_record") => {
                    let p = self.a.tuple_fields(f[3])?.to_vec();
                    if p.len() != 2 { return Err("native record declaration arity".into()); }
                    let name = self.atom_name(p[0])?;
                    let fields = self.a.list_items(p[1])?;
                    self.native.insert(name, Definition::Local(fields));
                }
                _ => (),
            }
        }
        for &id in input {
            let f = self.a.tuple_fields(id)?.to_vec();
            if self.a.tag(id) != Some("attribute") || f.len() != 4 || self.a.as_atom(f[2]) != Some("import_record") { continue; }
            let p = self.a.tuple_fields(f[3])?.to_vec();
            if p.len() != 2 { return Err("native record import arity".into()); }
            let module = self.atom_name(p[0])?;
            for name in self.a.list_items(p[1])? {
                self.native.insert(self.atom_name(name)?, Definition::Imported(module.clone()));
            }
        }
        Ok(())
    }
    fn native_id(&mut self, id: TermId) -> Result<Option<TermId>, String> {
        if let Some(name) = self.a.as_atom(id).map(str::to_owned) {
            return Ok(match self.native.get(&name).cloned() {
                Some(Definition::Local(_)) => Some(id),
                Some(Definition::Imported(module)) => {
                    let module = self.a.atom(module);
                    Some(self.a.tuple(vec![module, id]))
                }
                None => None,
            });
        }
        if matches!(self.a.get(id), ErlangTerm::Nil) { return Ok(Some(id)); }
        let fields = self.a.tuple_fields(id)?;
        if fields.len() == 2 && fields.iter().all(|&id| self.a.as_atom(id).is_some()) { return Ok(Some(id)); }
        Err("native record identifier must be a name, module/name or []".into())
    }
    fn native_pairs(&mut self, pairs: TermId, pattern: bool) -> Result<TermId, String> {
        let mut out = Vec::new();
        for pair in self.a.list_items(pairs)? {
            let mut f = self.a.tuple_fields(pair)?.to_vec();
            if self.a.tag(pair) != Some("record_field") || f.len() != 4 { return Err("native record field shape".into()); }
            f[3] = self.expr(f[3], pattern)?;
            out.push(self.a.tuple(f));
        }
        Ok(self.a.list(out))
    }
    fn native_defaults(&mut self, anno: TermId, definitions: Vec<TermId>, supplied: TermId) -> Result<TermId, String> {
        let mut overrides = BTreeMap::new();
        for pair in self.a.list_items(supplied)? {
            let p = self.a.tuple_fields(pair)?.to_vec();
            if p.len() != 4 { return Err("native record initializer shape".into()); }
            let key = self.a.tuple_fields(p[2])?[2];
            overrides.entry(self.atom_name(key)?).or_insert(p[3]);
        }
        let mut out = Vec::new();
        for mut field in definitions {
            if self.a.tag(field) == Some("typed_record_field") { field = self.a.tuple_fields(field)?[1]; }
            let f = self.a.tuple_fields(field)?.to_vec();
            if self.a.tag(field) != Some("record_field") || !matches!(f.len(), 3 | 4) { return Err("native record declaration field shape".into()); }
            let key = self.a.tuple_fields(f[2])?[2];
            let name = self.atom_name(key)?;
            let value = if let Some(value) = overrides.get(&name) { Some(*value) }
                else if f.len() == 4 { Some(self.reanno(f[3], anno)?) } else { None };
            if let Some(value) = value {
                // Fields without defaults are absent in Dialyzer's prepared
                // record. Unknown supplied fields are similarly filtered out.
                let field_anno = if f.len() == 4 { anno } else { f[1] };
                let key = if f.len() == 4 { self.lit_atom(anno, &name) } else { f[2] };
                out.push(self.a.tagged("record_field", vec![field_anno, key, value]));
            }
        }
        Ok(self.a.list(out))
    }
    pub(super) fn native_expression(&mut self, fields: &[TermId], pattern: bool) -> Result<Option<TermId>, String> {
        let tag = self.a.as_atom(fields[0]).map(str::to_owned);
        let tag = tag.as_deref();
        if tag == Some("record_field") && fields.len() == 4 {
            let mut f = fields.to_vec();
            f[3] = self.expr(f[3], pattern)?;
            return Ok(Some(self.a.tuple(f)));
        }
        if tag == Some("record") && fields.len() == 4 {
            let Some(mut id) = self.native_id(fields[2])? else { return Ok(None); };
            let mut pairs = fields[3];
            if !pattern {
                let name = self.a.as_atom(id).map(str::to_owned).or_else(|| {
                    self.a.tuple_fields(id).ok().and_then(|p| if self.a.as_atom(p[0]) == Some("shell_default") { self.a.as_atom(p[1]).map(str::to_owned) } else { None })
                });
                if let Some(name) = name {
                    if let Some(Definition::Local(definitions)) = self.native.get(&name).cloned() {
                        pairs = self.native_defaults(fields[1], definitions, pairs)?;
                        let module = self.a.atom(self.module.clone());
                        let name = self.a.atom(name);
                        id = self.a.tuple(vec![module, name]);
                    }
                }
            }
            let pairs = self.native_pairs(pairs, pattern)?;
            return Ok(Some(self.a.tagged("record", vec![fields[1], id, pairs])));
        }
        if matches!(tag, Some("record" | "record_field")) && fields.len() == 5 {
            let Some(id) = self.native_id(fields[3])? else { return Ok(None); };
            let source = self.expr(fields[2], false)?;
            let anno = self.first_anno(source)?;
            let tail = if tag == Some("record") { self.native_pairs(fields[4], false)? } else { fields[4] };
            return Ok(Some(self.a.tagged(tag.unwrap(), vec![anno, source, id, tail])));
        }
        Ok(None)
    }
}
