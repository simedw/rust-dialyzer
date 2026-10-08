// SPDX-License-Identifier: Apache-2.0
use super::*;
impl Lower {
    pub(super) fn info(
        &mut self,
        module: Id,
        line: Id,
        public: Vec<Id>,
        macros: Vec<Id>,
        map: Id,
    ) -> Result<(Id, Id)> {
        let zero = self.int(0);
        let one = self.int(1);
        let empty = self.list(vec![]);
        let public = self.list(public);
        let macros = self.list(macros);
        let structure = match self.optional(map, "struct") {
            Some(x) => x,
            None => self.atom("nil"),
        };
        let md5_input = self.tuple(vec![public, macros, structure]);
        let mut md5_arena = self.a.clone();
        md5_arena.root = md5_input;
        let bytes = rust_dialyzer_poc::external_term::encode(&md5_arena, Default::default(), false)
            .map_err(|e| e.to_string())?;
        let digest = self.add(T::Binary(md5(&bytes).to_vec()));
        let allowed = [
            "attributes",
            "compile",
            "functions",
            "macros",
            "md5",
            "exports_md5",
            "module",
            "deprecated",
            "struct",
        ];
        let mut literals = vec![];
        for k in allowed {
            let k = self.atom(k);
            literals.push(self.ast("atom", line, vec![k]))
        }
        let literals = self.list(literals);
        let union_name = self.atom("union");
        let union = self.ast("type", line, vec![union_name, literals]);
        let product_name = self.atom("product");
        let product_list = self.list(vec![union]);
        let product = self.ast("type", line, vec![product_name, product_list]);
        let any_name = self.atom("any");
        let any = self.ast("type", line, vec![any_name, empty]);
        let fun_args = self.list(vec![product, any]);
        let fun_name = self.atom("fun");
        let fun_type = self.ast("type", line, vec![fun_name, fun_args]);
        let info_name = self.atom("__info__");
        let na = self.tuple(vec![info_name, one]);
        let types = self.list(vec![fun_type]);
        let value = self.tuple(vec![na, types]);
        let spec_key = self.atom("spec");
        let spec = self.ast("attribute", line, vec![spec_key, value]);
        let deprecated = match self.optional(map, "deprecated") {
            Some(x) => x,
            None => empty,
        };
        let mut clauses = vec![];
        for (key, value) in [
            ("module", module),
            ("functions", public),
            ("macros", macros),
            ("struct", structure),
            ("exports_md5", digest),
        ] {
            let k = self.atom(key);
            let arg = self.ast("atom", zero, vec![k]);
            let args = self.list(vec![arg]);
            let value = self.lit(value, zero)?;
            let body = self.list(vec![value]);
            clauses.push(self.ast("clause", zero, vec![args, empty, body]))
        }
        for key in ["attributes", "compile", "md5"] {
            let key = self.atom(key);
            let atom = self.ast("atom", zero, vec![key]);
            let key_name = self.atom("Key");
            let var = self.ast("var", zero, vec![key_name]);
            let pattern = self.ast("match", zero, vec![var, atom]);
            let patterns = self.list(vec![pattern]);
            let erl = self.atom("erlang");
            let erl = self.ast("atom", zero, vec![erl]);
            let get = self.atom("get_module_info");
            let get = self.ast("atom", zero, vec![get]);
            let remote = self.ast("remote", zero, vec![erl, get]);
            let module = self.ast("atom", zero, vec![module]);
            let args = self.list(vec![module, var]);
            let call = self.ast("call", zero, vec![remote, args]);
            let body = self.list(vec![call]);
            clauses.push(self.ast("clause", zero, vec![patterns, empty, body]))
        }
        let key = self.atom("deprecated");
        let arg = self.ast("atom", zero, vec![key]);
        let args = self.list(vec![arg]);
        let value = self.lit(deprecated, zero)?;
        let body = self.list(vec![value]);
        clauses.push(self.ast("clause", zero, vec![args, empty, body]));
        let clauses = self.list(clauses);
        let info = self.ast("function", zero, vec![info_name, one, clauses]);
        Ok((spec, info))
    }
}
pub fn translate(input: &ErlangTerms, metadata: Id) -> Result<ErlangTerms> {
    translate_with_version(input, metadata, CompilerVersion::V1_20)
}
pub fn translate_with_version(input: &ErlangTerms, metadata: Id, compiler: CompilerVersion) -> Result<ErlangTerms> {
    validate_input(input, metadata)?;
    let mut l = Lower { a: input.clone(), compiler };
    let p = l.tup(metadata)?;
    if p.len() != 3 || l.name(p[0])? != "elixir_v1" {
        return Err("expected elixir_v1 metadata".into());
    }
    let map = p[1];
    let module = l.map(map, "module")?;
    let line = if let Some(a) = l.optional(map, "anno") {
        match l.get(a) {
            T::Integer(_) => a,
            T::Tuple(x) => x[0],
            _ => return Err("unsupported module annotation".into()),
        }
    } else {
        l.map(map, "line")?
    };
    let file = l.map(map, "relative_file")?;
    let chars = match l.get(file).clone() {
        T::Binary(bytes) => {
            let text = String::from_utf8(bytes).map_err(|_| "invalid relative file UTF-8")?;
            let xs = text.chars().map(|c| l.int(c as i64)).collect();
            l.list(xs)
        }
        _ => file,
    };
    let location = l.tuple(vec![chars, line]);
    let mut forms = vec![];
    for (key, value) in [("file", location), ("module", module)] {
        let key = l.atom(key);
        forms.push(l.ast("attribute", line, vec![key, value]))
    }
    let mut opts = vec![l.atom("no_auto_import")];
    for opt in l.items(l.map(map, "compile_opts")?)? {
        let name = match l.get(opt) {
            T::Tuple(v) => l.name(v[0]).ok(),
            T::Atom(s) => Some(s.clone()),
            _ => None,
        };
        if !matches!(name.as_deref(), Some("debug_info" | "no_warn_undefined")) {
            opts.push(opt)
        }
    }
    let opts = l.list(opts);
    let compile = l.atom("compile");
    forms.push(l.ast("attribute", line, vec![compile, opts]));
    forms.extend(l.items(p[2])?);
    let mut functions = vec![];
    let mut file_functions = vec![];
    let mut exports = vec![];
    let mut public = vec![];
    let mut macros = vec![];
    let unreachable = l.items(l.map(map, "unreachable")?)?;
    let mut unreachable_order = None;
    for def in l.items(l.map(map, "definitions")?)? {
        let d = l.tup(def)?;
        if d.len() != 4 {
            return Err("invalid definition".into());
        }
        let na = l.tup(d[0])?;
        if na.len() != 2 {
            return Err("definition identity must be {name,arity}".into());
        }
        if unreachable.iter().any(|u| {
            match unreachable_order
                .get_or_insert_with(|| rust_dialyzer_poc::term_order::TermOrderArena::new(input))
            {
                Ok(order) => order.compare_map_keys(*u, order, d[0]).is_ok_and(|o| o.is_eq()),
                Err(_) => false,
            }
        }) {
            continue;
        }
        let kind = l.name(d[1])?;
        let is_macro = matches!(kind.as_str(), "defmacro" | "defmacrop");
        if !matches!(kind.as_str(), "def" | "defp" | "defmacro" | "defmacrop") {
            return Err(format!("unsupported definition kind {kind}"));
        }
        let mut clauses = vec![];
        for cl in l.items(d[3])? {
            let cl = l.tup(cl)?;
            if cl.len() != 4 {
                return Err("invalid definition clause".into());
            }
            let mut s = Scope::default();
            let clmeta = if l
                .prop(cl[0], "line")?
                .is_some_and(|id| l.integer(id).ok() == l.integer(line).ok())
            {
                let generated = l.atom("generated");
                let yes = l.atom("true");
                let g = l.tuple(vec![generated, yes]);
                let linekey = l.atom("line");
                let zero = l.int(0);
                let loc = l.tuple(vec![linekey, zero]);
                l.list(vec![g, loc])
            } else {
                cl[0]
            };
            let c = l.clause(clmeta, cl[1], cl[2], cl[3], &mut s)?;
            if is_macro {
                let mut fields = l.tup(c)?;
                let mut args = l.items(fields[2])?;
                let name = l.atom("_@CALLER");
                let caller = l.ast("var", fields[1], vec![name]);
                args.insert(0, caller);
                fields[2] = l.list(args);
                if s.caller {
                    let ann = fields[1];
                    let n = l.atom("__CALLER__");
                    let target = l.ast("var", ann, vec![n]);
                    let source_name = l.atom("_@CALLER");
                    let source = l.ast("var", ann, vec![source_name]);
                    let remote = l.remote(ann, "elixir_env", "to_caller", vec![source]);
                    let assign = l.ast("match", ann, vec![target, remote]);
                    let mut body = l.items(fields[4])?;
                    body.insert(0, assign);
                    fields[4] = l.list(body)
                }
                clauses.push(l.tuple(fields))
            } else {
                clauses.push(c)
            }
        }
        let ann = l.ann(d[2])?;
        let name = if is_macro {
            let name = l.name(na[0])?;
            l.atom(&format!("MACRO-{name}"))
        } else {
            na[0]
        };
        let arity = if is_macro {
            l.int(l.integer(na[1])? + 1)
        } else {
            na[1]
        };
        let cs = l.list(clauses);
        let function = l.ast("function", ann, vec![name, arity, cs]);
        if let Some(file) = l.prop(d[2], "file")? {
            let parts = l.tup(file)?;
            if parts.len() != 2 {
                return Err("invalid function file metadata".into());
            }
            let chars = l.characters(parts[0])?;
            let value = l.tuple(vec![chars, parts[1]]);
            let key = l.atom("file");
            let nongenerated = if matches!(l.get(ann), T::List { .. }) {
                l.prop(ann, "location")?.ok_or("missing file location")?
            } else {
                ann
            };
            let attr = l.ast("attribute", nongenerated, vec![key, value]);
            file_functions.splice(0..0, [attr, function]);
        } else {
            functions.push(function)
        }
        if matches!(kind.as_str(), "def" | "defmacro") {
            exports.push(l.tuple(vec![name, arity]));
            if is_macro {
                macros.push(d[0])
            } else {
                public.push(d[0])
            }
        }
    }
    for attr in l.items(l.map(map, "attributes")?)? {
        let pair = l.tup(attr)?;
        forms.push(l.ast("attribute", line, pair))
    }
    public.sort_by_key(|id| {
        let p = l.tup(*id).unwrap();
        (l.name(p[0]).unwrap(), l.integer(p[1]).unwrap())
    });
    macros.sort_by_key(|id| {
        let p = l.tup(*id).unwrap();
        (l.name(p[0]).unwrap(), l.integer(p[1]).unwrap())
    });
    let info_name = l.atom("__info__");
    let one = l.int(1);
    exports.push(l.tuple(vec![info_name, one]));
    exports.sort_by_key(|id| {
        let p = l.tup(*id).unwrap();
        (l.name(p[0]).unwrap(), l.integer(p[1]).unwrap())
    });
    let exports = l.list(exports);
    let export = l.atom("export");
    forms.push(l.ast("attribute", line, vec![export, exports]));
    let (spec, info) = l.info(module, line, public, macros, map)?;
    forms.push(spec);
    forms.push(info);
    functions.reverse();
    forms.extend(functions);
    forms.extend(file_functions);
    l.a.root = l.list(forms);
    Ok(l.a)
}
