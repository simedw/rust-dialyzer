use crate::form_error::{FormError, MessagePiece, ReasonBuilder};
use crate::forms::{Builtin, DeclarationKind, FormId, Forms, TypeForm};
use crate::lattice::{
    AtomsView, MapEntry, MapKey, NominalName, Opacity, Presence, Type, TypeView, VariableId,
};
use crate::native_error::NativeTransferError;
use crate::program::{ModuleId, TypeName};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::collections::{BTreeMap, BTreeSet};
type Result<T> = std::result::Result<T, FormError>;
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Unfold {
    Named(TypeName),
    Record(String),
    #[cfg(otp_domain = "29")]
    NativeRecord { module: String, name: String },
}
#[derive(Clone, Debug)]
pub enum Site {
    Spec,
    Type(TypeName),
    Record,
    #[cfg(otp_domain = "29")]
    NativeRecord,
    Check,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RemotePolicy {
    #[default]
    Resolve,
    ReplaceByNone,
}

#[derive(Clone, Copy)]
struct ExpansionBudget {
    depth: i64,
    limit: i64,
}

#[derive(Clone, Default)]
struct State {
    variables: BTreeMap<VariableId, Type>,
    stack: Stack,
    massage: Option<Massage>,
    remote_policy: RemotePolicy,
    check_mode: bool,
    native_site: Option<crate::native_form_environment::NativeFormSite>,
}
/// The enclosing expansions, innermost first (OTP's list of recursion
/// names). A shared list, so entering an expansion and snapshotting the
/// state are O(1); each node keeps the length and a hash of its suffix.
#[derive(Clone, Default)]
struct Stack(Option<std::sync::Arc<StackNode>>);
struct StackNode {
    head: Unfold,
    tail: Stack,
    len: usize,
    hash: u64,
}
impl Stack {
    fn from_vec(items: Vec<Unfold>) -> Self {
        items.into_iter().rev().fold(Self::default(), |stack, item| stack.push(item))
    }
    fn push(&self, head: Unfold) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = crate::fast_hash::FxHasher::default();
        head.hash(&mut hasher);
        self.hash().hash(&mut hasher);
        Self(Some(std::sync::Arc::new(StackNode {
            head,
            tail: self.clone(),
            len: self.len() + 1,
            hash: hasher.finish(),
        })))
    }
    fn len(&self) -> usize {
        self.0.as_ref().map_or(0, |node| node.len)
    }
    fn hash(&self) -> u64 {
        self.0.as_ref().map_or(0, |node| node.hash)
    }
    fn iter(&self) -> impl Iterator<Item = &Unfold> {
        std::iter::successors(self.0.as_deref(), |node| node.tail.0.as_deref()).map(|node| &node.head)
    }
    fn contains(&self, item: &Unfold) -> bool {
        self.iter().any(|x| x == item)
    }
    fn to_vec(&self) -> Vec<Unfold> {
        self.iter().cloned().collect()
    }
}
impl PartialEq for Stack {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                std::sync::Arc::ptr_eq(a, b)
                    || (a.len == b.len && a.hash == b.hash && self.iter().eq(other.iter()))
            }
            _ => false,
        }
    }
}
impl Eq for Stack {}
/// The set dialyzer_utils:massage_type rewrites: aliases of `module` declared
/// before position `below`. Tested against `Forms::declarations` on demand;
/// declarations loaded later carry `usize::MAX` metadata, so they never join.
#[derive(Clone)]
struct Massage {
    module: String,
    below: usize,
    /// The materialized set, kept only under `verify_fast_paths`.
    eager: Option<std::sync::Arc<BTreeSet<TypeName>>>,
}
#[derive(Clone)]
struct Cached {
    arguments: Vec<Type>,
    value: Type,
    cost: i64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FormCacheEntry {
    pub name: TypeName,
    pub enclosing: Vec<Unfold>,
    pub depth: i64,
    pub arguments: Vec<Type>,
    pub value: Type,
    pub cost: i64,
}
#[derive(Clone, Debug)]
pub struct ModuleTableHandle {
    arena: std::sync::Arc<crate::core::ErlangTerms>,
    root: crate::core::TermId,
}
impl ModuleTableHandle {
    pub fn raw(&self) -> crate::native_form_environment::RawFormTerm {
        crate::native_form_environment::RawFormTerm::new(self.arena.clone(), self.root)
            .expect("validated module table")
    }

    pub fn new(arena: std::sync::Arc<crate::core::ErlangTerms>) -> Result<Self> {
        let root = arena.root;
        Self::at(arena, root)
    }
    pub fn at(
        arena: std::sync::Arc<crate::core::ErlangTerms>,
        root: crate::core::TermId,
    ) -> Result<Self> {
        if !matches!(
            arena.terms.get(root.0),
            Some(crate::core::ErlangTerm::Map(_))
        ) {
            return Err("module table root must be a map".into());
        }
        Ok(Self { arena, root })
    }
    pub fn root(&self) -> crate::core::TermId {
        self.root
    }
    pub fn arena(&self) -> &crate::core::ErlangTerms {
        &self.arena
    }
    pub fn shared_arena(&self) -> &std::sync::Arc<crate::core::ErlangTerms> {
        &self.arena
    }
}
/// Persistent map: `named` snapshots the cache before resolving arguments and
/// may restore it (as OTP threads its cache), so clones must be O(1) and
/// inserts must not copy the whole table.
/// Keys and values are shared: the path copies after a snapshot clone every
/// entry of the copied nodes, and deep-copying names and stacks dominated.
/// Keys order by a hash first, so a lookup compares names and stacks only
/// on a hash match; `FormCache::entries` restores the name order.
type CachedTypes = im::OrdMap<std::sync::Arc<CacheKey>, std::sync::Arc<Vec<Cached>>>;
#[derive(Clone)]
struct CacheKey {
    hash: u64,
    name: TypeName,
    stack: Stack,
    depth: i64,
}
impl CacheKey {
    fn new(name: TypeName, stack: Stack, depth: i64) -> Self {
        use std::hash::{Hash, Hasher};
        let mut hasher = crate::fast_hash::FxHasher::default();
        name.hash(&mut hasher);
        depth.hash(&mut hasher);
        stack.hash().hash(&mut hasher);
        Self {
            hash: hasher.finish(),
            name,
            stack,
            depth,
        }
    }
    /// The order of the `(name, enclosing, depth)` tuples OTP keys by.
    fn native_cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.name
            .cmp(&other.name)
            .then_with(|| self.stack.iter().cmp(other.stack.iter()))
            .then(self.depth.cmp(&other.depth))
    }
}
impl PartialEq for CacheKey {
    fn eq(&self, other: &Self) -> bool {
        self.hash == other.hash
            && self.depth == other.depth
            && self.name == other.name
            && self.stack == other.stack
    }
}
impl Eq for CacheKey {}
impl PartialOrd for CacheKey {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for CacheKey {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        if self == other {
            return std::cmp::Ordering::Equal;
        }
        self.hash.cmp(&other.hash).then_with(|| self.native_cmp(other))
    }
}
fn push_cached(types: &mut CachedTypes, key: CacheKey, cached: Cached) {
    match types.get_mut(&key) {
        Some(values) => std::sync::Arc::make_mut(values).push(cached),
        None => {
            types.insert(std::sync::Arc::new(key), std::sync::Arc::new(vec![cached]));
        }
    }
}

#[derive(Clone)]
pub struct FormCache {
    types: CachedTypes,
    module_tables: std::sync::Arc<BTreeMap<String, ModuleTableHandle>>,
    interop_complete: bool,
}
impl Default for FormCache {
    fn default() -> Self {
        Self {
            types: Default::default(),
            module_tables: Default::default(),
            interop_complete: true,
        }
    }
}
impl FormCache {
    pub fn new() -> Self {
        Self {
            interop_complete: true,
            ..Self::default()
        }
    }
    pub fn entries(&self) -> Vec<FormCacheEntry> {
        let mut keys = self.types.iter().collect::<Vec<_>>();
        keys.sort_by(|(a, _), (b, _)| a.native_cmp(b));
        keys.into_iter()
            .flat_map(|(key, values)| {
                let enclosing = key.stack.to_vec();
                values.iter().map(move |v| FormCacheEntry {
                    name: key.name.clone(),
                    enclosing: enclosing.clone(),
                    depth: key.depth,
                    arguments: v.arguments.clone(),
                    value: v.value.clone(),
                    cost: v.cost,
                })
            })
            .collect()
    }
    pub fn cached_tables(&self) -> &BTreeMap<String, ModuleTableHandle> {
        &self.module_tables
    }
    pub fn interop_complete(&self) -> bool {
        self.interop_complete
    }
}
#[derive(Clone)]
struct RecordOverride {
    name: String,
    form: FormId,
    source_key: Option<crate::native_form_environment::RawFormTerm>,
}
#[derive(Clone)]
pub(crate) struct OrderingInput {
    root: crate::native_form_environment::RawFormTerm,
    validation: std::sync::Arc<std::sync::OnceLock<Result<()>>>,
}
impl OrderingInput {
    pub(crate) fn new(root: crate::native_form_environment::RawFormTerm) -> Self {
        Self { root, validation: std::sync::Arc::new(std::sync::OnceLock::new()) }
    }
    fn validate(&self) -> Result<()> {
        let check = || {
            crate::term_order::RootTermOrder::new(self.root.arena(), self.root.id())?;
            Ok(())
        };
        let cached = self.validation.get_or_init(check);
        if crate::lattice::verify_fast_paths() {
            let fresh = check();
            assert_eq!(fresh.as_ref().err().map(ToString::to_string),
                       cached.as_ref().err().map(ToString::to_string),
                       "cached form ordering validation differs");
        }
        cached.clone()
    }
}
#[derive(Clone)]
pub struct Resolver<'a> {
    pub forms: std::borrow::Cow<'a, Forms>,
    pub modules: std::borrow::Cow<'a, [String]>,
    pub missing: Vec<TypeName>,
    pub external_types: Vec<crate::native_form_environment::ExternalTypeEvent>,
    native_site: Option<crate::native_form_environment::NativeFormSite>,
    cache: FormCache,
    record_types: BTreeMap<(ModuleId, String), Vec<Type>>,
    #[cfg(otp_domain = "29")]
    native_record_types: BTreeMap<(String,String),BTreeMap<String,Type>>,
    source_tables: Option<BTreeMap<String, ModuleTableHandle>>,
    ordering_inputs: Option<(Vec<OrderingInput>, std::sync::OnceLock<Result<()>>)>,
    exported_types: Option<BTreeSet<TypeName>>,
    definitions: BTreeMap<TypeName, crate::native_form_environment::RawDefinition>,
    compiled_sources: BTreeMap<ModuleId, crate::compiled_definitions::FormSourceArena>,
}
impl<'a> Resolver<'a> {
    pub fn new(forms: &'a Forms, modules: &'a [String]) -> Self {
        Self {
            forms: std::borrow::Cow::Borrowed(forms),
            modules: std::borrow::Cow::Borrowed(modules),
            missing: vec![],
            external_types: vec![],
            native_site: None,
            cache: FormCache::new(),
            record_types: BTreeMap::new(),
            #[cfg(otp_domain = "29")]
            native_record_types: forms.native_record_cached_types.clone(),
            source_tables: None,
            ordering_inputs: None,
            exported_types: None,
            definitions: BTreeMap::new(),
            compiled_sources: BTreeMap::new(),
        }
    }
    /// A resolver for a worker thread that borrows this one's forms and module
    /// names (owned or not) instead of copying them. Starts with no events and
    /// a fresh cache; if it ever has to modify the forms, they become its own.
    pub(crate) fn borrowing_worker(&self) -> Resolver<'_> {
        Resolver {
            forms: std::borrow::Cow::Borrowed(&*self.forms),
            modules: std::borrow::Cow::Borrowed(&*self.modules),
            missing: Vec::new(),
            external_types: Vec::new(),
            native_site: self.native_site.clone(),
            cache: FormCache::new(),
            record_types: self.record_types.clone(),
            #[cfg(otp_domain = "29")]
            native_record_types: self.native_record_types.clone(),
            source_tables: self.source_tables.clone(),
            ordering_inputs: self.ordering_inputs.clone(),
            exported_types: self.exported_types.clone(),
            definitions: self.definitions.clone(),
            compiled_sources: self.compiled_sources.clone(),
        }
    }
    pub(crate) fn set_source_tables(&mut self, tables: BTreeMap<String, ModuleTableHandle>) {
        self.source_tables = Some(tables);
    }
    pub(crate) fn set_exported_types(&mut self, exports: BTreeSet<TypeName>) {
        self.exported_types = Some(exports);
    }
    pub(crate) fn set_native_site(&mut self, site: crate::native_form_environment::NativeFormSite) {
        self.native_site = Some(site);
    }
    fn external_type(
        &mut self,
        name: &TypeName,
        origin: Option<FormId>,
        state: &State,
    ) -> Result<()> {
        if let Some(site) = &state.native_site {
            let raw = origin
                .and_then(|id| self.raw_form(id))
                .ok_or("external type lacks original annotation source")?;
            let crate::core::ErlangTerm::Tuple(fields) = raw.term() else {
                return Err(FormError::Native(NativeTransferError::FunctionClause));
            };
            let annotation = crate::native_form_environment::RawFormTerm {
                arena: raw.arena.clone(),
                id: fields[1],
            };
            let location = crate::native_form_environment::annotation_location(&annotation)?;
            self.external_types
                .push(crate::native_form_environment::ExternalTypeEvent {
                    target: name.clone(),
                    site: site.clone(),
                    location,
                });
        }
        self.missing.push(name.clone());
        Ok(())
    }
    pub(crate) fn set_ordering_inputs(
        &mut self,
        roots: Vec<OrderingInput>,
    ) {
        self.ordering_inputs = Some((roots, std::sync::OnceLock::new()));
    }
    fn validate_record_ordering(
        &self,
        raw: &crate::native_form_environment::RawFormTerm,
    ) -> Result<()> {
        if let Some((roots, validation)) = &self.ordering_inputs {
            validation.get_or_init(|| {
                // Roots validate independently and are cached per table, so
                // the first use validates the missing ones on all cores; the
                // first failure in root order is the sequential result.
                let missing = roots.iter().filter(|root| root.validation.get().is_none()).count();
                if missing > 8 {
                    crate::ordered_parallel::setup_map(roots, |root| root.validate().err())
                        .into_iter()
                        .flatten()
                        .next()
                        .map_or(Ok(()), Err)
                } else {
                    for root in roots {
                        root.validate()?;
                    }
                    Ok(())
                }
            }).clone()
        } else {
            crate::term_order::TermOrderArena::new(raw.arena())?;
            Ok(())
        }
    }
    fn compiled_environment(
        &self,
        module: &str,
    ) -> Option<crate::native_form_environment::LazyFormEnvironment> {
        let table = self.source_tables.as_ref()?.get(module)?;
        Some(crate::native_form_environment::LazyFormEnvironment {
            arena: table.arena.clone(),
            tables: BTreeMap::from([(module.to_owned(), table.root())]),
        })
    }
    fn native_record_environment(
        &self,
        module: ModuleId,
        name: &str,
    ) -> Option<crate::native_form_environment::LazyFormEnvironment> {
        if self
            .forms
            .records
            .get(&(module, name.to_owned()))
            .is_some_and(|r| r.metadata != usize::MAX)
        {
            return None;
        }
        self.compiled_environment(&self.modules[module.0])
    }
    fn missing_native_module(&self, name: &str) -> bool {
        self.source_tables
            .as_ref()
            .is_some_and(|tables| !tables.contains_key(name))
    }
    fn native_declaration(
        &self,
        name: &TypeName,
    ) -> Option<crate::native_form_environment::RawDeclaration> {
        self.compiled_environment(&name.module)
            .and_then(|source| source.declaration(name))
    }
    fn has_declaration(&self, name: &TypeName) -> bool {
        self.forms.declarations.contains_key(name) || self.native_declaration(name).is_some()
    }
    fn load_declaration(&mut self, name: &TypeName) -> Result<()> {
        if self.forms.declarations.contains_key(name) {
            return Ok(());
        }
        let Some(raw) = self.native_declaration(name) else {
            return Ok(());
        };
        let definition = raw.checked_definition()?;
        let module = ModuleId(
            self.modules
                .iter()
                .position(|n| n == &name.module)
                .ok_or("declaration lookup scope missing module")?,
        );
        let body = self.forms.to_mut().defer_raw(module, &definition.body);
        self.forms.to_mut().declarations.insert(
            name.clone(),
            crate::forms::Declaration {
                name: name.clone(),
                kind: raw.kind,
                parameters: Vec::new(),
                body,
                exported: self
                    .exported_types
                    .as_ref()
                    .is_some_and(|e| e.contains(name)),
                metadata: usize::MAX,
            },
        );
        self.definitions.insert(name.clone(), definition);
        Ok(())
    }
    pub(crate) fn materialize(&mut self, id: FormId) -> Result<()> {
        if !matches!(self.forms.nodes[id.0].value, TypeForm::DeferredRaw) {
            return Ok(());
        }
        let mid = self.forms.nodes[id.0].module;
        let arena = self.forms.source_arena(id)
            .or_else(|| self.compiled_sources.get(&mid).map(|source| source.terms.clone()))
            .ok_or("deferred form lacks its source arena")?;
        let view = crate::forms::FormSource {
            name: &self.modules[mid.0],
            terms: &arena.terms,
        };
        self.forms
            .to_mut()
            .materialize(&view, id)
            .map_err(|_| FormError::Native(NativeTransferError::FunctionClause))
    }
    pub(crate) fn set_compiled_sources(
        &mut self,
        sources: BTreeMap<ModuleId, crate::compiled_definitions::FormSourceArena>,
        definitions: BTreeMap<TypeName, crate::native_form_environment::RawDefinition>,
    ) {
        self.compiled_sources = sources;
        self.definitions = definitions;
    }
    pub(crate) fn raw_form(
        &self,
        id: FormId,
    ) -> Option<crate::native_form_environment::RawFormTerm> {
        let form = &self.forms.nodes[id.0];
        let arena = self.forms.source_arena(id)
            .or_else(|| self.compiled_sources.get(&form.module).map(|source| source.terms.clone()))?;
        Some(crate::native_form_environment::RawFormTerm {
            arena,
            id: form.raw,
        })
    }
    fn throw_message(&self, pieces: Vec<MessagePiece>) -> FormError {
        FormError::Throw(ReasonBuilder::new().message(pieces))
    }
    fn record_field_error_at(&self, state: &State, record: &str, field: &str) -> FormError {
        #[cfg(otp_domain = "29")]
        if let Some(site) = &state.native_site
            && matches!(site.kind, crate::native_form_environment::NativeSiteKind::NativeRecord) {
            return self.native_record_message(&site.scope.named().module, record, Some(field));
        }
        #[cfg(not(otp_domain = "29"))]
        let _ = state;
        self.illegal_record_field(record, field)
    }
    fn illegal_record_field(&self, record: &str, field: &str) -> FormError {
        let context = crate::lattice::PresentationContext::default();
        self.throw_message(vec![
            MessagePiece::Text("Illegal declaration of #".into()),
            MessagePiece::Printed(context.atom_to_string(record)),
            MessagePiece::Text("{".into()),
            MessagePiece::Printed(context.atom_to_string(field)),
            MessagePiece::Text("}\n".into()),
        ])
    }
    fn unable_evaluate(&self, id: FormId) -> FormError {
        let Some(raw) = self.raw_form(id) else {
            return FormError::Transport(
                "native evaluation failure lacks original source arena".into(),
            );
        };
        let orders = self
            .compiled_sources
            .get(&self.forms.nodes[id.0].module)
            .map(|s| &s.iterator_orders);
        let written = match orders {
            Some(orders) => crate::native_form_io::write_with_map_orders(
                &raw,
                crate::native_form_io::Encoding::Latin1,
                orders,
            ),
            None => crate::native_form_io::write(&raw, crate::native_form_io::Encoding::Latin1),
        };
        match written {
            Ok(written) => self.throw_message(vec![
                MessagePiece::Text("Unable to evaluate type ".into()),
                MessagePiece::RawWritten(written),
                MessagePiece::Text("\n".into()),
            ]),
            Err(error) => FormError::Transport(error),
        }
    }
    fn touch_module(&mut self, name: &str) {
        if self.cache.module_tables.contains_key(name) {
            return;
        }
        match &self.source_tables {
            Some(tables) => {
                if let Some(table) = tables.get(name) {
                    std::sync::Arc::make_mut(&mut self.cache.module_tables)
                        .insert(name.into(), table.clone());
                }
            }
            None => self.cache.interop_complete = false,
        }
    }
    pub fn cache(&self) -> &FormCache {
        &self.cache
    }
    pub fn replace_cache(&mut self, cache: FormCache) -> FormCache {
        std::mem::replace(&mut self.cache, cache)
    }
    #[cfg(otp_domain = "29")]
    pub(crate) fn set_native_record_types(&mut self, types: BTreeMap<(String,String),BTreeMap<String,Type>>) { self.native_record_types=types; }
    pub fn record_prototype(&self, module: ModuleId, name: &str) -> Option<&[Type]> {
        self.record_types
            .get(&(module, name.into()))
            .map(Vec::as_slice)
    }
    pub fn set_record_types(
        &mut self,
        types: BTreeMap<(ModuleId, String), Vec<Type>>,
    ) -> Result<()> {
        for (key, fields) in &types {
            let record = self
                .forms
                .records
                .get(key)
                .ok_or("record prototype has no declaration")?;
            if record.fields.len() != fields.len() {
                return Err("record prototype field arity mismatch".into());
            }
        }
        self.record_types = types;
        Ok(())
    }
    pub fn finalize_records(&mut self, modules: &BTreeSet<ModuleId>) -> Result<()> {
        let records = self
            .forms
            .records
            .values()
            .filter(|r| modules.contains(&r.module))
            .cloned()
            .collect::<Vec<_>>();
        for record in records {
            let fields = record
                .fields
                .iter()
                .map(|field| match field.declared_type {
                    Some(id) => self.resolve(id, Site::Record, BTreeMap::new()),
                    None => Ok(Type::any()),
                })
                .collect::<Result<Vec<_>>>()?;
            self.record_types
                .insert((record.module, record.name), fields);
        }
        Ok(())
    }
    pub fn check_declarations(&mut self, modules: &BTreeSet<ModuleId>) -> Result<()> {
        let mut roots = self
            .forms
            .declarations
            .values()
            .filter(|d| {
                self.modules
                    .iter()
                    .position(|m| m == &d.name.module)
                    .is_some_and(|i| modules.contains(&ModuleId(i)))
            })
            .map(|d| d.body)
            .collect::<Vec<_>>();
        roots.extend(
            self.forms
                .records
                .values()
                .filter(|r| modules.contains(&r.module))
                .flat_map(|r| r.fields.iter().filter_map(|f| f.declared_type)),
        );
        for id in roots {
            self.check_record_fields(id, &BTreeMap::new())?;
        }
        Ok(())
    }
    #[cfg(otp_domain = "29")]
    fn check_native_record_fields(&mut self, current: ModuleId, module: &str, name: &str, ids: &[FormId], qualified: bool, variables: &BTreeMap<VariableId, Type>) -> Result<()> {
        use crate::native_form_environment::{NativeFormSite,NativeSiteKind};
        if qualified && self.modules[current.0] != module {
            return Err(FormError::Native(NativeTransferError::BadMatchNative(Box::new(crate::lattice::NativeValue::Atom(self.modules[current.0].clone())))));
        }
        if self.missing_native_module(module) || (!self.modules.iter().any(|m|m == module) && self.compiled_environment(module).is_none()) {
            return Err(self.native_record_message(module,name,None));
        }
        // The qualified branch caches its lookup; the import branch discards that cache.
        if qualified { self.touch_module(module); }
        let raw=self.compiled_environment(module).map(|e|e.native_record_fields(module,name)).transpose()?.flatten();
        let compiled=self.forms.native_records.iter().find(|((_,m,n),_)|m == module && n == name).map(|(_,r)|r.clone());
        if raw.is_none() && compiled.is_none() { return Err(FormError::Native(NativeTransferError::BadMatchNative(Box::new(crate::lattice::NativeValue::Atom("error".into()))))); }
        let arity=if let Some(raw)=&raw {raw.record_arity()?} else {compiled.as_ref().unwrap().fields.len()};
        for (index,id) in ids.iter().enumerate() {
            self.materialize(*id)?;
            let TypeForm::Builtin{name:Builtin::FieldType,arguments:Some(fields)}=self.forms.nodes[id.0].value.clone() else {return Err(FormError::Native(NativeTransferError::FunctionClause))};
            if fields.len()!=2 {return Err(FormError::Native(NativeTransferError::FunctionClause))}
            self.materialize(fields[0])?;
            let TypeForm::Atom(field)=self.forms.nodes[fields[0].0].value.clone() else {return Err(FormError::Native(NativeTransferError::FunctionClause))};
            let previous=self.native_site.clone();
            if let Some(site)=&previous {
                let native=!qualified && index == 0;
                self.native_site=Some(NativeFormSite { kind:if native {NativeSiteKind::NativeRecord} else {NativeSiteKind::Record},
                    scope:TypeName {module:if native {module.into()} else {self.modules[current.0].clone()},name:if qualified {"ok".into()} else {name.into()},arity:if native {0} else {arity}}.into(),file:site.file.clone()});
            }
            let value=self.resolve(fields[1],Site::Record,variables.clone());self.native_site=previous;
            let value=value?.substitute(&BTreeMap::new())?;
            let declared=self.native_record_types.get(&(module.into(),name.into())).and_then(|f|f.get(&field));
            let impossible=if let Some(declared)=declared {value.meet(declared)?.is_impossible()} else if let Some(raw)=&raw {
                let cached=raw.cached_field_type(&field)?;
                crate::native_type_operand::NativeTypeOperand::Canonical(value).inf(&crate::native_type_operand::NativeTypeOperand::from_raw(cached))?.is_impossible()
            } else {
                if !compiled.as_ref().unwrap().fields.iter().any(|f|f.name == field) {return Err(FormError::Native(NativeTransferError::BadMatchBoolean(false)))}
                value.is_impossible()
            };
            if impossible { return Err(if qualified {self.native_record_message(module,name,Some(&field))} else {self.illegal_record_field(name,&field)}); }
        }
        Ok(())
    }
    pub fn check_record_fields(
        &mut self,
        id: FormId,
        variables: &BTreeMap<VariableId, Type>,
    ) -> Result<()> {
        let mut work = vec![id];
        while let Some(id) = work.pop() {
            self.materialize(id)?;
            let node = self.forms.nodes[id.0].clone();
            if let TypeForm::Builtin {
                name: Builtin::Fun | Builtin::Record,
                arguments: Some(ids),
            } = &node.value
                && let Some(first) = ids.first()
            {
                self.materialize(*first)?;
            }
            match &node.value {
                #[cfg(otp_domain = "29")]
                TypeForm::QualifiedRecordName { .. } => return Err(FormError::Native(NativeTransferError::FunctionClause)),
                TypeForm::DeferredRaw => {
                    return Err("deferred form survived materialization".into());
                }
                TypeForm::Variable(_)
                | TypeForm::Atom(_)
                | TypeForm::Integer(_)
                | TypeForm::Operator { .. } => {}
                TypeForm::Annotated { body, .. } | TypeForm::Parenthesized(body) => {
                    work.push(*body)
                }
                TypeForm::Named { arguments, .. }
                | TypeForm::UnrecognizedBuiltin {
                    arguments: Some(arguments),
                    ..
                } => work.extend(arguments.iter().rev()),
                TypeForm::UnrecognizedBuiltin {
                    arguments: None, ..
                } => return Err(FormError::Native(NativeTransferError::FunctionClause)),
                TypeForm::BoundedFunction { .. } | TypeForm::Constraint { .. } => {
                    return Err(FormError::Native(NativeTransferError::FunctionClause));
                }
                TypeForm::Builtin {
                    name: Builtin::Binary | Builtin::Range,
                    arguments: Some(ids),
                } if ids.len() == 2 => {}
                TypeForm::Builtin {
                    name: Builtin::Record,
                    arguments: Some(ids),
                } => {
                    #[cfg(otp_domain = "29")]
                    {
                        if ids.is_empty() { continue; }
                        if let Some(TypeForm::QualifiedRecordName { module, name }) = ids.first().map(|id| self.forms.nodes[id.0].value.clone()) {
                            self.check_native_record_fields(node.module,&module,&name,&ids[1..],true,variables)?;
                            continue;
                        }
                    }
                    let Some(TypeForm::Atom(name)) =
                        ids.first().map(|id| self.forms.nodes[id.0].value.clone())
                    else {
                        return Err(FormError::Native(NativeTransferError::FunctionClause));
                    };
                    self.touch_module(&self.modules[node.module.0].clone());
                    let native_environment = self.native_record_environment(node.module, &name);
                    let native_record = native_environment
                        .as_ref()
                        .map(|env| env.record_fields(&self.modules[node.module.0], &name))
                        .transpose()?
                        .flatten();
                    #[cfg(otp_domain = "29")]
                    if native_record.is_none() && !self.forms.records.contains_key(&(node.module,name.clone())) {
                        let owner=self.native_record_module(node.module,&name)?;
                        self.check_native_record_fields(node.module,&owner,&name,&ids[1..],false,variables)?;
                        continue;
                    }
                    let record = if native_environment.is_some() {
                        if native_record.is_none() {
                            return Err(FormError::Native(NativeTransferError::BadMatchNative(
                                Box::new(crate::lattice::NativeValue::Atom("error".into())),
                            )));
                        }
                        None
                    } else {
                        Some(
                            self.forms
                                .records
                                .get(&(node.module, name.clone()))
                                .ok_or_else(|| {
                                    FormError::Native(NativeTransferError::BadMatchNative(
                                        Box::new(crate::lattice::NativeValue::Atom("error".into())),
                                    ))
                                })?
                                .clone(),
                        )
                    };
                    for id in &ids[1..] {
                        self.materialize(*id)?;
                        let form = self.forms.nodes[id.0].value.clone();
                        let TypeForm::Builtin {
                            name: Builtin::FieldType,
                            arguments: Some(fields),
                        } = &form
                        else {
                            return Err(FormError::Native(NativeTransferError::FunctionClause));
                        };
                        if fields.len() != 2 {
                            return Err(FormError::Native(NativeTransferError::FunctionClause));
                        }
                        self.materialize(fields[0])?;
                        let TypeForm::Atom(field) = self.forms.nodes[fields[0].0].value.clone()
                        else {
                            return Err(FormError::Native(NativeTransferError::FunctionClause));
                        };
                        let field = field.clone();
                        let body = fields[1];
                        let previous_site = self.native_site.clone();
                        if let Some(site) = &previous_site {
                            self.native_site =
                                Some(crate::native_form_environment::NativeFormSite {
                                    kind: crate::native_form_environment::NativeSiteKind::Record,
                                    scope: TypeName {
                                        module: self.modules[node.module.0].clone(),
                                        name: name.clone(),
                                        arity: match &native_record {
                                            Some(raw) => raw.record_arity()?,
                                            None => record
                                                .as_ref()
                                                .expect("compiled record")
                                                .fields
                                                .len(),
                                        },
                                    }
                                    .into(),
                                    file: site.file.clone(),
                                });
                        }
                        let resolved = self.resolve(body, Site::Record, variables.clone());
                        self.native_site = previous_site;
                        let value = resolved?.substitute(&BTreeMap::new())?;
                        let impossible = match &native_record {
                            Some(raw) => {
                                let cached = raw.cached_field_type(&field)?;
                                crate::native_type_operand::NativeTypeOperand::Canonical(
                                    value.clone(),
                                )
                                .inf(&crate::native_type_operand::NativeTypeOperand::from_raw(
                                    cached,
                                ))?
                                .is_impossible()
                            }
                            None => {
                                let position = record
                                    .as_ref()
                                    .expect("compiled record")
                                    .fields
                                    .iter()
                                    .position(|f| f.name == field)
                                    .ok_or(FormError::Native(
                                        NativeTransferError::BadMatchBoolean(false),
                                    ))?;
                                let declared = self
                                    .record_types
                                    .get(&(node.module, name.clone()))
                                    .and_then(|types| types.get(position))
                                    .cloned()
                                    .unwrap_or_else(Type::any);
                                value.meet(&declared)?.is_impossible()
                            }
                        };
                        if impossible {
                            return Err(self.illegal_record_field(&name, &field));
                        }
                    }
                }
                TypeForm::Builtin {
                    name: Builtin::Fun,
                    arguments: Some(ids),
                } if ids.len() == 2
                    && matches!(
                        self.forms.nodes[ids[0].0].value,
                        TypeForm::Builtin {
                            name: Builtin::Any,
                            arguments: None
                        }
                    ) =>
                {
                    work.push(ids[1])
                }
                TypeForm::Builtin {
                    arguments: Some(ids),
                    ..
                } => work.extend(ids.iter().rev()),
                TypeForm::Builtin {
                    name: Builtin::Tuple | Builtin::Map,
                    arguments: None,
                } => {}
                TypeForm::Builtin {
                    arguments: None, ..
                } => return Err(FormError::Native(NativeTransferError::FunctionClause)),
            }
        }
        Ok(())
    }
    pub fn resolve(
        &mut self,
        id: FormId,
        site: Site,
        variables: BTreeMap<VariableId, Type>,
    ) -> Result<Type> {
        if matches!(site, Site::Check) {
            return Err(FormError::Native(NativeTransferError::FunctionClause));
        }
        let stack = if let Site::Type(name) = site {
            Stack::default().push(Unfold::Named(name))
        } else {
            Stack::default()
        };
        let state = State {
            variables,
            stack,
            massage: None,
            remote_policy: RemotePolicy::Resolve,
            check_mode: false,
            native_site: self.native_site.clone(),
        };
        self.resolve_state(id, state)
    }
    /// Native t_from_form_without_remote policy; no external arguments are expanded.
    pub fn resolve_without_remote(&mut self, id: FormId, site: Site) -> Result<Type> {
        if matches!(site, Site::Check) {
            return Err(FormError::Native(NativeTransferError::FunctionClause));
        }
        let stack = if let Site::Type(name) = site {
            Stack::default().push(Unfold::Named(name))
        } else {
            Stack::default()
        };
        self.resolve_state(
            id,
            State {
                variables: BTreeMap::new(),
                stack,
                massage: None,
                remote_policy: RemotePolicy::ReplaceByNone,
                check_mode: false,
                native_site: self.native_site.clone(),
            },
        )
    }
    /// Native t_from_form_check_remote starts with no recursion names and uses unlimited budgets.
    pub fn check_remote(&mut self, id: FormId) -> Result<()> {
        self.check_remote_at(id, Site::Spec)
    }
    pub fn check_remote_at(&mut self, id: FormId, site: Site) -> Result<()> {
        let state = State {
            check_mode: matches!(site, Site::Check),
            native_site: self.native_site.clone(),
            ..State::default()
        };
        self.resolve_state_with_budget(id, state, 1 << 25, 1 << 25)?;
        Ok(())
    }
    pub fn resolve_contract(
        &mut self,
        id: FormId,
        metadata: usize,
        variables: BTreeMap<VariableId, Type>,
        massage: bool,
    ) -> Result<Type> {
        let module = self.modules[self.forms.nodes[id.0].module.0].clone();
        // OTP rewrites redefined built-in type names everywhere except module
        // `erlang`, which redefines them all for documentation
        // (dialyzer_utils:core_to_attr_tuples/1).
        let massage = (massage && module != "erlang").then(|| self.massage(module, metadata));
        self.resolve_state(
            id,
            State {
                variables,
                stack: Stack::default(),
                massage,
                remote_policy: RemotePolicy::Resolve,
                check_mode: false,
                native_site: self.native_site.clone(),
            },
        )
    }
    fn massage(&self, module: String, below: usize) -> Massage {
        let eager = crate::lattice::verify_fast_paths().then(|| {
            std::sync::Arc::new(
                self.forms
                    .declarations
                    .values()
                    .filter(|d| {
                        d.name.module == module
                            && d.kind == DeclarationKind::Alias
                            && d.metadata < below
                    })
                    .map(|d| d.name.clone())
                    .collect(),
            )
        });
        Massage { module, below, eager }
    }
    fn massaged(&self, massage: &Option<Massage>, target: &TypeName) -> bool {
        #[cfg(otp_domain = "29")]
        if !crate::native_form_preprocessing::is_builtin_type(&target.name, target.arity) { return false; }
        let Some(m) = massage else {
            return false;
        };
        let lazy = target.module == m.module
            && self.forms.declarations.get(target).is_some_and(|d| {
                d.kind == DeclarationKind::Alias && d.metadata < m.below
            });
        if let Some(eager) = &m.eager {
            assert_eq!(eager.contains(target), lazy, "massage membership for {target:?}");
        }
        lazy
    }
    fn resolve_state(&mut self, id: FormId, state: State) -> Result<Type> {
        self.resolve_state_with_budget(id, state, 16, 10000)
    }
    fn resolve_state_with_budget(
        &mut self,
        id: FormId,
        state: State,
        depth: i64,
        limit: i64,
    ) -> Result<Type> {
        let (value, left) = self.form(id, &state, depth, limit)?;
        if left > 0 {
            return Ok(value);
        }
        let (mut value, _) = self.form(id, &state, 1, limit)?;
        for depth in 2..=4 {
            let (candidate, left) = self.form(id, &state, depth, limit)?;
            if left <= 0 {
                break;
            }
            value = candidate;
            if (limit - left) * 8 > limit {
                break;
            }
        }
        Ok(value)
    }
    fn list(&mut self, ids: &[FormId], s: &State, d: i64, mut l: i64) -> Result<(Vec<Type>, i64)> {
        let mut values = vec![];
        for id in ids {
            let (v, left) = self.form(*id, s, d, l - 1)?;
            values.push(v);
            l = left
        }
        Ok((values, l))
    }
    fn join(values: Vec<Type>) -> Result<Type> {
        values
            .into_iter()
            .try_fold(Type::none(), |a, b| a.join(&b).map_err(FormError::from))
    }
    fn eval_integer(&mut self, id: FormId) -> Result<BigInt> {
        self.materialize(id)?;
        match self.forms.nodes[id.0].value.clone() {
            TypeForm::Integer(n) => Ok(n),
            TypeForm::Operator { name, arguments } => {
                let left = self.eval_integer(arguments[0])?;
                match (name.as_str(), arguments.len()) {
                    ("+", 1) => Ok(left),
                    ("-", 1) => Ok(-left),
                    ("bnot", 1) => Ok(!left),
                    (_, 2) => {
                        let right = self.eval_integer(arguments[1])?;
                        Ok(match name.as_str() {
                            "+" => left + right,
                            "-" => left - right,
                            "*" => left * right,
                            "div" if !right.is_zero() => left / right,
                            "rem" if !right.is_zero() => left % right,
                            "band" => left & right,
                            "bor" => left | right,
                            "bxor" => left ^ right,
                            "bsl" | "bsr" => {
                                let n =
                                    right.to_i64().ok_or("type constant shift resource limit")?;
                                let n = if name == "bsr" { -n } else { n };
                                if n.unsigned_abs() > 1_000_000 {
                                    return Err("type constant shift resource limit".into());
                                }
                                if n >= 0 {
                                    left << (n as usize)
                                } else {
                                    left >> ((-n) as usize)
                                }
                            }
                            _ => return Err("Unable to evaluate type operator".into()),
                        })
                    }
                    _ => Err("Unable to evaluate type operator".into()),
                }
            }
            _ => Err("Unable to evaluate type integer expression".into()),
        }
    }
    fn named(
        &mut self,
        name: &TypeName,
        remote: bool,
        args: &[FormId],
        s: &State,
        budget: ExpansionBudget,
        origin: Option<FormId>,
    ) -> Result<(Type, i64)> {
        let ExpansionBudget { depth: d, limit: l } = budget;
        if remote && s.remote_policy == RemotePolicy::ReplaceByNone {
            return Ok((Type::none(), l));
        }
        if remote {
            let known = self.source_tables.as_ref().map_or_else(
                || self.modules.contains(&name.module),
                |m| m.contains_key(&name.module),
            );
            if !known {
                self.external_type(name, origin, s)?;
                return Ok((Type::any(), l));
            }
            self.touch_module(&name.module);
            let exported = self.exported_types.as_ref().map_or_else(
                || {
                    self.forms
                        .declarations
                        .get(name)
                        .is_some_and(|d| d.exported)
                },
                |e| e.contains(name),
            );
            if !exported {
                self.external_type(name, origin, s)?;
                return Ok((Type::any(), l));
            }
        }
        let unfold = Unfold::Named(name.clone());
        if s.stack.iter().filter(|n| *n == &unfold).count() >= 2 {
            return Ok((Type::any(), l));
        }
        if !remote && self.missing_native_module(&name.module) {
            let Some(raw) = origin.and_then(|id| self.raw_form(id)) else {
                return Err(FormError::Transport(
                    "missing local module lacks original call AST".into(),
                ));
            };
            let crate::core::ErlangTerm::Tuple(fields) = raw.term() else {
                return Err(FormError::Native(NativeTransferError::FunctionClause));
            };
            let (name_id, args_id) = (fields[2], fields[3]);
            let mut builder = ReasonBuilder::new();
            let name = builder.copy(&crate::native_form_environment::RawFormTerm {
                arena: raw.arena.clone(),
                id: name_id,
            });
            let args = builder.copy(&crate::native_form_environment::RawFormTerm {
                arena: raw.arena.clone(),
                id: args_id,
            });
            let reason = builder.tuple(vec![name, args]);
            return Err(FormError::ErrorReason(builder.finish(reason)));
        }
        if !remote && self.modules.contains(&name.module) {
            self.touch_module(&name.module);
        }
        if s.check_mode
            && let Some(raw) = self.native_declaration(name)
        {
            raw.checked_shape()?;
            let (_, left) = self.list(args, s, d, l)?;
            return Ok((Type::any(), left));
        }
        self.load_declaration(name)?;
        let decl = self
            .forms
            .declarations
            .get(name)
            .ok_or_else(|| {
                let context = crate::lattice::PresentationContext::default();
                let pieces = if remote {
                    vec![
                        MessagePiece::Text("Unable to find remote type ".into()),
                        MessagePiece::Printed(context.atom_latin1(&name.module)),
                        MessagePiece::Text(":".into()),
                        MessagePiece::Printed(context.atom_to_string(&name.name)),
                        MessagePiece::Text("()\n".into()),
                    ]
                } else {
                    vec![
                        MessagePiece::Text("Unable to find type ".into()),
                        MessagePiece::Printed(context.atom_to_string(&name.name)),
                        MessagePiece::Text("/".into()),
                        MessagePiece::Printed(name.arity.to_string()),
                        MessagePiece::Text("\n".into()),
                    ]
                };
                self.throw_message(pieces)
            })?
            .clone();
        if s.check_mode {
            let (_, left) = self.list(args, s, d, l)?;
            return Ok((Type::any(), left));
        }
        let mut state = s.clone();
        state.stack = s.stack.push(unfold.clone());
        if let (Some(site), Some(definition)) = (&s.native_site, self.definitions.get(name)) {
            state.native_site = Some(crate::native_form_environment::NativeFormSite {
                kind: crate::native_form_environment::NativeSiteKind::Type,
                scope: name.clone().into(),
                file: definition.file.clone(),
            });
            let _ = site;
        }
        let before_arguments = self.cache.clone();
        #[cfg(otp_domain = "29")]
        let definition_site = std::mem::replace(&mut state.native_site, s.native_site.clone());
        let (arguments, left) = self.list(args, &state, d, l)?;
        #[cfg(otp_domain = "29")]
        { state.native_site = definition_site; }
        let cache_name = if remote {
            name.clone()
        } else {
            TypeName {
                module: self
                    .definitions
                    .get(name)
                    .map_or_else(|| name.module.clone(), |def| def.module.clone()),
                name: name.name.clone(),
                arity: name.arity,
            }
        };
        let cache_key = CacheKey::new(cache_name, s.stack.clone(), d);
        if let Some(hit) = self
            .cache
            .types
            .get(&cache_key)
            .and_then(|values| values.iter().find(|c| c.arguments == arguments))
            .cloned()
        {
            self.cache = before_arguments;
            return Ok((hit.value.clone(), if remote { l } else { left } - hit.cost));
        }
        drop(before_arguments);
        let parameters = match self.definitions.get(name) {
            Some(definition) => definition.parameter_names(arguments.len())?,
            None => decl.parameters.clone(),
        };
        if parameters.len() != arguments.len() {
            return Err(FormError::Native(NativeTransferError::FunctionClause));
        }
        state.variables = parameters
            .into_iter()
            .zip(arguments.iter().cloned())
            .collect();
        state.massage = (decl.kind == DeclarationKind::Alias && name.module != "erlang")
            .then(|| self.massage(name.module.clone(), decl.metadata));
        let recursive = s.stack.contains(&unfold);
        let limited = recursive && !(left <= 2 && d <= 10);
        let (value, body_left) = self.form(
            decl.body,
            &state,
            if limited { 2 } else { d },
            if limited { 10 } else { left },
        )?;
        let remainder = if limited { left - body_left } else { body_left };
        let value = match decl.kind {
            DeclarationKind::Alias => value,
            kind => Type::nominal(
                NominalName::Defined {
                    module: self
                        .definitions
                        .get(name)
                        .map_or_else(|| name.module.clone(), |d| d.module.clone()),
                    name: name.name.clone(),
                    arity: name.arity,
                    opacity: if kind == DeclarationKind::Opaque {
                        Opacity::Opaque
                    } else {
                        Opacity::Transparent
                    },
                },
                value,
            ),
        };
        let cost = left - remainder;
        if cost >= 0 {
            push_cached(
                &mut self.cache.types,
                cache_key,
                Cached {
                    arguments,
                    value: value.clone(),
                    cost,
                },
            );
        }
        Ok((value, remainder))
    }
    fn form(&mut self, id: FormId, s: &State, d: i64, l: i64) -> Result<(Type, i64)> {
        if d <= 0 || l <= 0 {
            return Ok((Type::any(), l));
        }
        self.materialize(id)?;
        let node = self.forms.nodes[id.0].clone();
        match node.value.clone() {
            TypeForm::Variable(name) => Ok((
                if name == VariableId::Atom("_".into()) {
                    Type::any()
                } else {
                    s.variables
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| match name {
                            VariableId::Atom(v) => Type::variable_atom(v),
                            VariableId::Integer(v) => Type::variable_id(VariableId::Integer(v)),
                        })
                },
                l,
            )),
            TypeForm::Atom(a) => Ok((Type::atom(a), l)),
            TypeForm::Integer(n) => Ok((Type::integer(n), l)),
            TypeForm::Operator { .. } => match self.eval_integer(id) {
                Ok(value) => Ok((Type::integer(value), l)),
                Err(_) => Err(self.unable_evaluate(id)),
            },
            TypeForm::Annotated { body, .. } | TypeForm::Parenthesized(body) => {
                self.form(body, s, d, l)
            }
            TypeForm::Named {
                name,
                remote,
                arguments,
            } => self.named(
                &name,
                remote,
                &arguments,
                s,
                ExpansionBudget { depth: d, limit: l },
                Some(id),
            ),
            TypeForm::Builtin { name, arguments } => self.builtin(
                node.module,
                name,
                arguments.as_deref(),
                s,
                ExpansionBudget { depth: d, limit: l },
                id,
            ),
            _ => Err(FormError::Native(NativeTransferError::FunctionClause)),
        }
    }
    fn builtin(
        &mut self,
        module: ModuleId,
        name: Builtin,
        args: Option<&[FormId]>,
        s: &State,
        budget: ExpansionBudget,
        origin: FormId,
    ) -> Result<(Type, i64)> {
        let ExpansionBudget { depth: d, limit: l } = budget;
        use Builtin::*;
        let ids = args.unwrap_or(&[]);
        let target = TypeName {
            module: self.modules[module.0].clone(),
            name: name.name().into(),
            arity: ids.len(),
        };
        if args.is_some() && self.massaged(&s.massage, &target) {
            let mut state = s.clone();
            state.massage = Option::None;
            return self.named(
                &target,
                false,
                ids,
                &state,
                ExpansionBudget { depth: d, limit: l },
                Option::None,
            );
        }
        if name == Any && args.is_none() {
            return Err(FormError::Native(NativeTransferError::FunctionClause));
        }

        if name == Map && args.is_none() {
            self.touch_module(&self.modules[module.0].clone());
        }
        let range = |a: &str, b: &str| Type::range_strings(a, b);
        let unary = matches!(name, List | NonemptyList) && ids.len() == 1;
        if unary {
            let (v, left) = self.form(ids[0], s, if name == List { d - 1 } else { d }, l - 1)?;
            return Ok((Type::proper_list(v, name == NonemptyList), left));
        }
        if matches!(
            name,
            MaybeImproperList | NonemptyMaybeImproperList | NonemptyImproperList
        ) && ids.len() == 2
        {
            let (a, left) = self.form(ids[0], s, d, l - 1)?;
            let (b, left) = self.form(ids[1], s, d, left)?;
            let value = if name == NonemptyImproperList {
                Type::cons(a, b)?
            } else {
                Type::list(a, b, name == NonemptyMaybeImproperList)
            };
            return Ok((value, left));
        }
        match name {
            Fun if ids.len() == 2 => {
                self.materialize(ids[0])?;
                let (domain, left) = match self.forms.nodes[ids[0].0].value.clone() {
                    TypeForm::Builtin {
                        name: Product,
                        arguments: Some(domain),
                    } => {
                        let (v, l) = self.list(&domain, s, d, l)?;
                        (Some(v), l)
                    }
                    TypeForm::Builtin { name: Any, .. } => (Option::None, l - 1),
                    _ => return Err(FormError::Native(NativeTransferError::FunctionClause)),
                };
                let (result, left) =
                    self.form(ids[1], s, if domain.is_none() { d - 1 } else { d }, left)?;
                return Ok((Type::function(domain, result), left));
            }
            Product | Tuple | Union if args.is_some() => {
                let (values, left) = self.list(ids, s, if name == Union { d } else { d - 1 }, l)?;
                return Ok((
                    match name {
                        Product => Type::product(values),
                        Tuple => Type::tuple(values)?,
                        _ => Self::join(values)?,
                    },
                    left,
                ));
            }
            Range if ids.len() == 2 => {
                let (Ok(from), Ok(to)) = (self.eval_integer(ids[0]), self.eval_integer(ids[1]))
                else {
                    return Err(self.unable_evaluate(origin));
                };
                return Ok((
                    Type::range(
                        crate::lattice::Endpoint::Finite(from),
                        crate::lattice::Endpoint::Finite(to),
                    )?,
                    l,
                ));
            }
            Binary if ids.len() == 2 => {
                let (Ok(base), Ok(unit)) = (self.eval_integer(ids[0]), self.eval_integer(ids[1]))
                else {
                    return Err(self.unable_evaluate(origin));
                };
                if base.is_negative() || unit.is_negative() {
                    return Err(self.unable_evaluate(origin));
                }
                return Ok((Type::bitstring(unit, base)?, l));
            }
            #[cfg(otp_domain = "29")]
            Record if args.is_none() => return Err(FormError::Native(NativeTransferError::FunctionClause)),
            Record => return self.record(module, ids, s, d, l),
            Map if args.is_some() => return self.map(ids, s, d, l),
            _ => {}
        }
        if !ids.is_empty() {
            return Err(FormError::Native(NativeTransferError::FunctionClause));
        }
        let value = match name {
            Any | Dynamic | Term => Type::any(),
            Arity => range("0", "255")?,
            Atom | Module | Node => Type::all_atoms(),
            Binary => Type::binary(),
            Bitstring => Type::bitstring(1.into(), 0.into())?,
            Bool | Boolean => Type::atoms(["false", "true"]),
            Byte => range("0", "255")?,
            Char => range("0", "1114111")?,
            Float => Type::float(),
            Function | Fun => Type::function(Option::None, Type::any()),
            Identifier => Type::identifier(Option::None)?,
            Integer => Type::all_integers(),
            Iolist | Iodata => {
                let tail = Type::binary().join(&Type::nil())?;
                let inner = Type::list(Type::any(), tail.clone(), false);
                let el = Self::join(vec![inner, Type::binary(), range("0", "255")?])?;
                let list = Type::list(el, tail, false);
                if name == Iodata {
                    list.join(&Type::binary())?
                } else {
                    list
                }
            }
            List => Type::proper_list(Type::any(), false),
            Map => {
                let local = TypeName {
                    module: self.modules[module.0].clone(),
                    name: "map".into(),
                    arity: 0,
                };
                if self.has_declaration(&local) {
                    return self.named(
                        &local,
                        false,
                        &[],
                        s,
                        ExpansionBudget { depth: d, limit: l },
                        Option::None,
                    );
                }
                Type::map_any()
            }
            Mfa => Type::tuple(vec![
                Type::all_atoms(),
                Type::all_atoms(),
                range("0", "255")?,
            ])?,
            Nil => Type::nil(),
            NegInteger => range("neg_inf", "-1")?,
            NonNegInteger => range("0", "pos_inf")?,
            NoReturn => Type::unit(),
            None => Type::none(),
            NonemptyBinary | NonemptyBitstring => {
                let local = TypeName {
                    module: self.modules[module.0].clone(),
                    name: name.name().into(),
                    arity: 0,
                };
                if self.has_declaration(&local) {
                    return self.named(
                        &local,
                        false,
                        &[],
                        s,
                        ExpansionBudget { depth: d, limit: l },
                        Option::None,
                    );
                }
                Type::bitstring(
                    if name == NonemptyBinary { 8 } else { 1 }.into(),
                    if name == NonemptyBinary { 8 } else { 1 }.into(),
                )?
            }
            NonemptyList => Type::proper_list(Type::any(), true),
            NonemptyMaybeImproperList => Type::cons(Type::any(), Type::any())?,
            NonemptyString | String => {
                Type::proper_list(range("0", "1114111")?, name == NonemptyString)
            }
            Number => Type::number(),
            Pid | Port | Reference => Type::identifier(Some(vec![
                match name {
                    Pid => "pid",
                    Port => "port",
                    _ => "reference",
                }
                .into(),
            ]))?,
            PosInteger => range("1", "pos_inf")?,
            MaybeImproperList => Type::list(Type::any(), Type::any(), false),
            Timeout => range("0", "pos_inf")?.join(&Type::atom("infinity"))?,
            Tuple => Type::tuple_any(),
            _ => return Err(FormError::Native(NativeTransferError::FunctionClause)),
        };
        Ok((value, l))
    }
    fn record_overrides(
        &mut self,
        ids: &[FormId],
        s: &State,
        depth: i64,
        mut left: i64,
    ) -> Result<(Vec<RecordOverride>, i64)> {
        let mut overrides = Vec::new();
        for id in ids {
            self.materialize(*id)?;
            let TypeForm::Builtin {
                name: Builtin::FieldType,
                arguments: Some(fields),
            } = self.forms.nodes[id.0].value.clone()
            else {
                return Err(FormError::Native(NativeTransferError::FunctionClause));
            };
            if fields.len() != 2 {
                return Err(FormError::Native(NativeTransferError::FunctionClause));
            }
            self.materialize(fields[0])?;
            let TypeForm::Atom(name) = self.forms.nodes[fields[0].0].value.clone() else {
                return Err(FormError::Native(NativeTransferError::FunctionClause));
            };
            let (_, remaining) = self.form(fields[1], s, depth, left - 1)?;
            left = remaining;
            // The key is the atom payload, not the surrounding atom AST.
            let key = self.raw_form(fields[0]).and_then(|raw| match raw.term() {
                crate::core::ErlangTerm::Tuple(v) => v.get(2).map(|id| crate::native_form_environment::RawFormTerm { arena: raw.arena.clone(), id: *id }),
                _ => None,
            });
            overrides.push(RecordOverride {
                name,
                form: fields[1],
                source_key: key,
            });
        }
        overrides.reverse();
        overrides.sort_by(|a, b| a.name.cmp(&b.name));
        Ok((overrides, left))
    }
    fn native_record_values(
        &mut self,
        module: ModuleId,
        raw: &crate::native_form_environment::RawFormTerm,
        ids: &[FormId],
        s: &State,
        depth: i64,
        left: i64,
    ) -> Result<(Vec<Type>, i64)> {
        use crate::core::{ErlangTerm, TermId};
        #[derive(Clone)]
        enum Entry {
            Raw(TermId),
            Modified(RecordOverride),
        }
        let originals = raw.record_entries()?;
        let tuple = |id: TermId| match &raw.arena.terms[id.0] {
            ErlangTerm::Tuple(v) => Some(v.as_slice()),
            _ => None,
        };
        let key = |e: &Entry| -> Result<crate::native_form_environment::RawFormTerm> {
            match e {
                Entry::Raw(id) => tuple(*id).and_then(|v|v.first().copied())
                    .map(|id|crate::native_form_environment::RawFormTerm {arena:raw.arena.clone(),id})
                    .ok_or(FormError::Native(NativeTransferError::Badarg)),
                Entry::Modified(entry) => entry.source_key.clone()
                    .ok_or_else(|| "native override lacks original atom payload".into()),
            }
        };
        self.validate_record_ordering(&raw)?;
        let normal = crate::term_order::RootTermOrder::new(&raw.arena,raw.id())?;
        let compare_keys = |a:&crate::native_form_environment::RawFormTerm,b:&crate::native_form_environment::RawFormTerm| -> Result<std::cmp::Ordering> {
            let left;let right;
            // `normal` only covers terms reachable from the record definition. Override keys
            // can share its arena while belonging to another form (e.g. `#r{f :: T}` in a spec).
            let reuse=|t:&crate::native_form_environment::RawFormTerm| std::sync::Arc::ptr_eq(t.arena(),&raw.arena)&&normal.contains(t.id());
            let a_order=if reuse(a){&normal}else{left=crate::term_order::RootTermOrder::new(a.arena(),a.id())?;&left};
            let b_order=if reuse(b){&normal}else{right=crate::term_order::RootTermOrder::new(b.arena(),b.id())?;&right};
            Ok(a_order.compare(a.id(),b_order,b.id())?)
        };
        let sort = |entries: &mut Vec<Entry>| -> Result<()> {
            for i in 1..entries.len() {
                let mut j = i;
                while j > 0 && compare_keys(&key(&entries[j])?,&key(&entries[j-1])?)?.is_lt() {
                    entries.swap(j,j-1);j-=1;
                }
            }
            Ok(())
        };
        let mut selected = originals
            .iter()
            .copied()
            .map(Entry::Raw)
            .collect::<Vec<_>>();
        let mut left = left;
        if !ids.is_empty() {
            sort(&mut selected)?;
            let (overrides, remaining) = self.record_overrides(ids, s, depth, left)?;
            left = remaining;
            let overrides = overrides
                .into_iter()
                .map(Entry::Modified)
                .collect::<Vec<_>>();
            let (mut di, mut mi) = (0, 0);
            let mut acc = Vec::new();
            while mi < overrides.len() {
                let Entry::Modified(entry) = &overrides[mi] else {
                    return Err("native modified entry invariant".into());
                };
                let name = &entry.name;
                let mkey = entry.source_key.as_ref().ok_or("native override lacks original atom payload")?;
                let valid = selected.get(di).and_then(|e| match e {
                    Entry::Raw(id) => tuple(*id).filter(|v| v.len() == 3),
                    _ => None,
                });
                let Some(header) = valid else {
                    return Err(self.record_field_error_at(s,
                        &s.native_site
                            .as_ref()
                            .ok_or("native record site missing")?
                            .scope
                            .named()
                            .name,
                        name,
                    ));
                };
                let order = compare_keys(&crate::native_form_environment::RawFormTerm {arena:raw.arena.clone(),id:header[0]},mkey)?;
                if order.is_eq() {
                    acc.push(overrides[mi].clone());
                    di += 1;
                    mi += 1;
                } else if order.is_lt() {
                    acc.push(selected[di].clone());
                    di += 1;
                } else {
                    return Err(self.record_field_error_at(s,
                        &s.native_site
                            .as_ref()
                            .ok_or("native record site missing")?
                            .scope
                            .named()
                            .name,
                        name,
                    ));
                }
            }
            let mut merged = selected[di..].to_vec();
            acc.reverse();
            merged.extend(acc);
            sort(&mut merged)?;
            let mut restored = Vec::new();
            for id in &originals {
                let Some(header) = tuple(*id).filter(|v| v.len() == 3) else {
                    continue;
                };
                let mut found = None;
                for entry in &merged {
                    let candidate = match entry {
                        Entry::Raw(id) => tuple(*id).and_then(|fields| fields.first().copied())
                            .map(|id| crate::native_form_environment::RawFormTerm { arena:raw.arena.clone(),id }),
                        Entry::Modified(entry) => Some(entry.source_key.clone()
                            .ok_or("native override lacks original atom payload")?),
                    };
                    if let Some(candidate) = candidate
                        && compare_keys(&crate::native_form_environment::RawFormTerm {arena:raw.arena.clone(),id:header[0]},&candidate)?.is_eq() {
                        found = Some(entry.clone());
                        break;
                    }
                }
                restored.push(found.ok_or(FormError::Native(NativeTransferError::FunctionClause))?);
            }
            selected = restored;
        }
        let mut values = Vec::new();
        let mut state = s.clone();
        state.variables.clear();
        for entry in selected {
            let mut field_state = state.clone();
            let id = match entry {
                Entry::Modified(entry) => entry.form,
                Entry::Raw(id) => {
                    field_state.massage = None;
                    let Some(v) = tuple(id).filter(|v| v.len() == 3) else {
                        return Err(FormError::Native(NativeTransferError::FunctionClause));
                    };
                    self.forms.to_mut().defer_raw(module, &crate::native_form_environment::RawFormTerm { arena: raw.arena.clone(), id: v[1] })
                }
            };
            let (value, remaining) = self.form(id, &field_state, depth, left)?;
            values.push(value);
            left = remaining;
        }
        Ok((values, left))
    }
    #[cfg(otp_domain = "29")]
    fn native_record_module(&self, module: ModuleId, name: &str) -> Result<String> {
        if let Some(import) = self.forms.native_record_imports.get(&(module, name.into())) {
            return Ok(import.clone());
        }
        Ok(self.compiled_environment(&self.modules[module.0])
            .map(|env| env.native_record_import(&self.modules[module.0], name))
            .transpose()?.flatten().unwrap_or_else(|| self.modules[module.0].clone()))
    }
    #[cfg(otp_domain = "29")]
    fn native_record_message(&self, module: &str, name: &str, field: Option<&str>) -> FormError {
        let context = crate::lattice::PresentationContext::default();
        let mut pieces = vec![MessagePiece::Text(if field.is_some() { "Illegal declaration of #" } else { "Unknown record #" }.into()),
            MessagePiece::Printed(context.atom_to_string(module)), MessagePiece::Text(":".into()),
            MessagePiece::Printed(context.atom_to_string(name)), MessagePiece::Text("{".into())];
        if let Some(field) = field { pieces.push(MessagePiece::Printed(context.atom_to_string(field))); }
        pieces.push(MessagePiece::Text("}\n".into()));
        self.throw_message(pieces)
    }
    #[cfg(otp_domain = "29")]
    fn qualified_record(&mut self, module: &str, name: &str, overrides: &[FormId], s: &State, d: i64, l: i64) -> Result<(Type, i64)> {
        use crate::lattice::{NativeRecordName, RecordField};
        let identity = NativeRecordName::Named { module: module.into(), name: name.into() };
        let unfold = Unfold::NativeRecord { module: module.into(), name: name.into() };
        let generic = || Type::record(identity.clone(), BTreeMap::new());
        if s.stack.iter().filter(|n| *n == &unfold).count() >= 2 { return Ok((generic(), l)); }
        let known = self.source_tables.as_ref().map_or_else(|| self.modules.iter().any(|m|m == module), |tables| tables.contains_key(module));
        if !known { return Ok((generic(), l)); }
        self.touch_module(module);
        let mid = match self.modules.iter().position(|m| m == module) {
            Some(i) => ModuleId(i),
            None => { let i = self.modules.len(); self.modules.to_mut().push(module.into()); ModuleId(i) }
        };
        let raw = self.compiled_environment(module).map(|env|env.native_record_fields(module,name)).transpose()?.flatten();
        let compiled = self.forms.native_records.get(&(mid,module.into(),name.into())).cloned();
        if raw.is_none() && compiled.is_none() { return Err(self.native_record_message(module,name,None)); }
        let recursive = s.stack.contains(&unfold) && !(l <= 2 && d <= 10);
        let mut state = s.clone();
        state.stack = s.stack.push(unfold);
        if let Some(site) = &s.native_site {
            state.native_site = Some(crate::native_form_environment::NativeFormSite {
                kind: crate::native_form_environment::NativeSiteKind::NativeRecord,
                scope: TypeName {module:module.into(),name:name.into(),arity:0}.into(),file:site.file.clone()
            });
        }
        let (depth,left) = if recursive {(2,10)} else {(d,l)};
        let (names,values,left) = if let Some(raw) = raw {
            let names = raw.record_fields()?.into_iter().map(|f|f.name).collect::<Vec<_>>();
            let (values,left) = self.native_record_values(mid,&raw,overrides,&state,depth,left)?;
            (names,values,left)
        } else {
            let record = compiled.expect("record definition");
            let (overrides,mut left) = self.record_overrides(overrides,&state,depth,left)?;
            for field in &overrides {
                if !record.fields.iter().any(|f| f.name == field.name) { return Err(self.native_record_message(module,name,Some(&field.name))); }
            }
            state.variables.clear();
            let mut names=Vec::new(); let mut values=Vec::new();
            for field in record.fields {
                let modified=overrides.iter().find(|e|e.name == field.name);
                let mut field_state=state.clone(); if modified.is_none() { field_state.massage=None; }
                let value=if let Some(id)=modified.map(|e|e.form).or(field.declared_type) {
                    let (value,remaining)=self.form(id,&field_state,depth,left)?;left=remaining;value
                } else { Type::any() };
                names.push(field.name); values.push(value);
            }
            (names,values,left)
        };
        let fields=names.into_iter().zip(values).map(|(name,value)|(name.into(),RecordField::Present(value))).collect();
        Ok((Type::record(identity,fields),if recursive {l-left} else {left}))
    }
    fn record(
        &mut self,
        module: ModuleId,
        ids: &[FormId],
        s: &State,
        d: i64,
        l: i64,
    ) -> Result<(Type, i64)> {
        #[cfg(otp_domain = "29")]
        let module = s.native_site.as_ref().and_then(|site| site.scope.module().ok()).and_then(|name| self.modules.iter().position(|m|m == name)).map(ModuleId).unwrap_or(module);
        if let Some(id) = ids.first() {
            self.materialize(*id)?;
        }
        #[cfg(otp_domain = "29")]
        {
            if ids.is_empty() { return Ok((Type::record_any(), l)); }
            if let Some(TypeForm::QualifiedRecordName { module, name }) = ids.first().map(|id| self.forms.nodes[id.0].value.clone()) {
                return self.qualified_record(&module, &name, &ids[1..], s, d, l);
            }
        }
        let Some(TypeForm::Atom(name)) = ids.first().map(|id| self.forms.nodes[id.0].value.clone())
        else {
            return Err(FormError::Native(NativeTransferError::FunctionClause));
        };
        let unfold = Unfold::Record(name.clone());
        if s.stack.iter().filter(|x| *x == &unfold).count() >= 2 {
            return Ok((Type::any(), l));
        }
        if self.missing_native_module(&self.modules[module.0]) {
            return Err(FormError::Native(NativeTransferError::BadMatchNative(
                Box::new(crate::lattice::NativeValue::Atom("error".into())),
            )));
        }
        self.touch_module(&self.modules[module.0].clone());
        let native = self
            .native_record_environment(module, &name)
            .as_ref()
            .map(|e| e.record_fields(&self.modules[module.0], &name))
            .transpose()?
            .flatten();
        let compiled = self.forms.records.get(&(module, name.clone())).cloned();
        #[cfg(otp_domain = "29")]
        if native.is_none() && compiled.is_none() {
            let owner = self.native_record_module(module, &name)?;
            return self.qualified_record(&owner, &name, &ids[1..], s, d, l);
        }
        if native.is_none() && compiled.is_none() {
            let context = crate::lattice::PresentationContext::default();
            return Err(self.throw_message(vec![
                MessagePiece::Text("Unknown record #".into()),
                MessagePiece::Printed(context.atom_to_string(&name)),
                MessagePiece::Text("{}\n".into()),
            ]));
        }
        if s.check_mode {
            return Ok((Type::any(), l));
        }
        let arity = match &native {
            Some(raw) => raw.record_arity()?,
            None => compiled.as_ref().expect("compiled record").fields.len(),
        };
        let recursive = s.stack.contains(&unfold) && !(l <= 2 && d <= 10);
        let mut state = s.clone();
        state.stack = s.stack.push(unfold);
        if let Some(site) = &s.native_site {
            state.native_site = Some(crate::native_form_environment::NativeFormSite {
                kind: crate::native_form_environment::NativeSiteKind::Record,
                scope: TypeName {
                    module: self.modules[module.0].clone(),
                    name: name.clone(),
                    arity,
                }
                .into(),
                file: site.file.clone(),
            });
        }
        let (depth, left) = if recursive { (2, 10) } else { (d, l) };
        let (fields, left) = if let Some(raw) = native {
            self.native_record_values(module, &raw, &ids[1..], &state, depth, left)?
        } else {
            let record = compiled.expect("compiled record");
            let (overrides, mut left) = self.record_overrides(&ids[1..], &state, depth, left)?;
            let mut declared = record
                .fields
                .iter()
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>();
            declared.sort();
            let mut position = 0;
            for entry in &overrides {
                let field = &entry.name;
                while position < declared.len() && declared[position] < field.as_str() {
                    position += 1;
                }
                if position == declared.len() || declared[position] != field {
                    return Err(self.illegal_record_field(&name, field));
                }
                position += 1;
            }
            state.variables.clear();
            let mut values = Vec::new();
            for field in record.fields {
                let modified = overrides.iter().find(|entry| entry.name == field.name);
                let id = modified.map(|entry| entry.form).or(field.declared_type);
                if let Some(id) = id {
                    let mut field_state = state.clone();
                    if modified.is_none() {
                        field_state.massage = None;
                    }
                    let (v, r) = self.form(id, &field_state, depth, left)?;
                    values.push(v);
                    left = r;
                } else {
                    values.push(Type::any());
                }
            }
            (values, left)
        };
        let mut values = vec![Type::atom(name)];
        values.extend(fields);
        Ok((
            Type::tuple(values)?,
            if recursive { l - left } else { left },
        ))
    }
    fn map(&mut self, ids: &[FormId], s: &State, d: i64, mut l: i64) -> Result<(Type, i64)> {
        let mut pairs = vec![];
        let mut remaining = ids.iter();
        loop {
            if l <= 0 {
                pairs.push((Type::any(), Presence::Optional, Type::any()));
                break;
            }
            let Some(id) = remaining.next() else {
                break;
            };
            self.materialize(*id)?;
            let form = self.forms.nodes[id.0].value.clone();
            let TypeForm::Builtin {
                name,
                arguments: Some(fields),
            } = &form
            else {
                return Err(FormError::Native(NativeTransferError::FunctionClause));
            };
            if fields.len() != 2 {
                return Err(FormError::Native(NativeTransferError::FunctionClause));
            }
            let presence = match name {
                Builtin::MapFieldAssoc => Presence::Optional,
                Builtin::MapFieldExact => Presence::Mandatory,
                _ => return Err(FormError::Native(NativeTransferError::FunctionClause)),
            };
            let (key, left) = self.form(fields[0], s, d - 1, l)?;
            let (value, left) = self.form(fields[1], s, d - 1, left)?;
            l = left - 1;
            // OTP's singleton_elements makes non-singleton required keys
            // optional before splitting their alternatives.
            let presence = if presence == Presence::Mandatory && MapKey::from_type(&key).is_err() {
                Presence::Optional
            } else {
                presence
            };
            for key in Self::separate_key(&key)? {
                pairs.push((key, presence, value.clone()))
            }
        }
        let mut shadow = vec![];
        let mut mandatory = vec![];
        let mut entries = BTreeMap::new();
        let mut dk = Type::none();
        let mut dv = Type::none();
        for (source, presence, value) in pairs {
            let mut key = source.clone();
            for prior in &shadow {
                key = key.subtract(prior)?
            }
            if !key.is_none() {
                shadow.push(key.clone())
            }
            if presence == Presence::Mandatory {
                if source.is_none() {
                    return Ok((Type::none(), l));
                }
                mandatory.push(source)
            }
            if let Ok(exact) = MapKey::from_type(&key) {
                entries.insert(
                    exact.clone(),
                    MapEntry {
                        key: exact,
                        presence: if value.is_none() {
                            Presence::Optional
                        } else {
                            presence
                        },
                        value,
                    },
                );
            } else if !key.is_none() && !value.is_none() {
                dk = dk.join(&key)?;
                dv = dv.join(&value)?
            }
        }
        for key in &mandatory {
            let mut possible = false;
            for prior in &shadow {
                possible |= !key.meet(prior)?.is_none()
            }
            if !possible {
                return Ok((Type::none(), l));
            }
        }
        for entry in entries.values_mut() {
            if mandatory.iter().any(|key| key == &entry.key.as_type()) {
                entry.presence = Presence::Mandatory
            }
        }
        Ok((Type::map(entries.into_values().collect(), dk, dv)?, l))
    }
    fn separate_key(key: &Type) -> Result<Vec<Type>> {
        if let TypeView::Families(slots) = key.view() {
            if slots.is_union_record {
                return key.elements().map_err(FormError::from).and_then(|parts| {
                    parts
                        .iter()
                        .map(Self::separate_key)
                        .collect::<Result<Vec<_>>>()
                        .map(|values| values.into_iter().flatten().collect())
                });
            }
            if let Some(AtomsView::Set(atoms)) = slots.atoms {
                return Ok(atoms.iter().map(Type::atom).collect());
            }
            if slots.number.is_some() {
                return key.elements().map_err(FormError::from);
            }
        }
        Ok(vec![key.clone()])
    }
}

impl FormCache {
    /// Restores cache records produced by the native exported API; metadata remains owned.
    pub fn from_native_arena(arena: std::sync::Arc<crate::core::ErlangTerms>) -> Result<Self> {
        use crate::core::{ErlangTerm, TermId};
        use num_traits::ToPrimitive;
        fn tuple(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId, n: usize) -> Result<&[TermId]> {
            match &terms[id.0] {
                ErlangTerm::Tuple(v) if v.len() == n => Ok(v),
                _ => Err("native cache tuple shape".into()),
            }
        }
        fn atom(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId) -> Result<String> {
            match &terms[id.0] {
                ErlangTerm::Atom(v) => Ok(v.clone()),
                _ => Err("native cache atom shape".into()),
            }
        }
        fn list(terms: &crate::shared_vec::SharedVec<ErlangTerm>, mut id: TermId) -> Result<Vec<TermId>> {
            let mut out = Vec::new();
            loop {
                match &terms[id.0] {
                    ErlangTerm::Nil => return Ok(out),
                    ErlangTerm::List { items, tail } => {
                        out.extend(items);
                        id = *tail;
                    }
                    _ => return Err("native cache proper list shape".into()),
                }
            }
        }
        fn number(terms: &crate::shared_vec::SharedVec<ErlangTerm>, id: TermId) -> Result<num_bigint::BigInt> {
            match &terms[id.0] {
                ErlangTerm::Integer(v) => {
                    v.parse().map_err(|_| "native cache integer shape".into())
                }
                _ => Err("native cache integer shape".into()),
            }
        }
        let terms = &arena.terms;
        let fields = tuple(terms, arena.root, 3)?;
        if atom(terms, fields[0])? != "cache" {
            return Err("expected native cache record".into());
        }
        let ErlangTerm::Map(entries) = &terms[fields[1].0] else {
            return Err("native cache types map shape".into());
        };
        let mut cache = Self::new();
        for (key, value) in entries {
            let key = tuple(terms, *key, 5)?;
            let value = tuple(terms, *value, 2)?;
            let arguments = list(terms, key[3])?
                .into_iter()
                .map(|id| crate::type_exports::type_from_arena(&arena, id).map_err(FormError::from))
                .collect::<Result<Vec<_>>>()?;
            let name = TypeName {
                module: atom(terms, key[0])?,
                name: atom(terms, key[1])?,
                arity: arguments.len(),
            };
            let depth = number(terms, key[2])?
                .to_i64()
                .ok_or("native cache depth outside generated API bounds")?;
            let cost = number(terms, value[1])?
                .to_i64()
                .ok_or("native cache cost outside generated API bounds")?;
            let mut enclosing = Vec::new();
            for scope in list(terms, key[4])? {
                let scope = tuple(terms, scope, 2)?;
                enclosing.push(match atom(terms, scope[0])?.as_str() {
                    "record" => Unfold::Record(atom(terms, scope[1])?),
                    #[cfg(otp_domain = "29")]
                    "native_record" => {
                        let pair = tuple(terms, scope[1], 2)?;
                        Unfold::NativeRecord { module: atom(terms, pair[0])?, name: atom(terms, pair[1])? }
                    }
                    "type" => {
                        let mfa = tuple(terms, scope[1], 3)?;
                        Unfold::Named(TypeName {
                            module: atom(terms, mfa[0])?,
                            name: atom(terms, mfa[1])?,
                            arity: number(terms, mfa[2])?
                                .to_usize()
                                .ok_or("native cache type arity outside generated API bounds")?,
                        })
                    }
                    _ => return Err("native cache recursion name shape".into()),
                });
            }
            let value = crate::type_exports::type_from_arena(&arena, value[0])?;
            push_cached(
                &mut cache.types,
                CacheKey::new(name, Stack::from_vec(enclosing), depth),
                Cached {
                    arguments,
                    value,
                    cost,
                },
            );
        }
        let ErlangTerm::Map(entries) = &terms[fields[2].0] else {
            return Err("native cache module map shape".into());
        };
        for (name, root) in entries {
            std::sync::Arc::make_mut(&mut cache.module_tables).insert(
                atom(terms, *name)?,
                ModuleTableHandle::at(arena.clone(), *root)?,
            );
        }
        Ok(cache)
    }
}

#[cfg(test)]
mod cache_key_tests {
    use super::*;
    fn name(module: &str, name: &str) -> TypeName {
        TypeName { module: module.into(), name: name.into(), arity: 0 }
    }
    fn cached(cost: i64) -> Cached {
        Cached { arguments: vec![], value: Type::any(), cost }
    }
    #[test]
    fn keys_match_by_value_and_entries_keep_the_native_order() {
        let inner = Unfold::Named(name("m", "a"));
        let outer = Unfold::Record("r".into());
        let built = Stack::default().push(outer.clone()).push(inner.clone());
        let parsed = Stack::from_vec(vec![inner.clone(), outer.clone()]);
        assert!(built == parsed && built.len() == 2 && built.hash() == parsed.hash());
        assert_eq!(built.to_vec(), [inner.clone(), outer.clone()]);
        assert!(built.contains(&outer) && !Stack::default().contains(&outer));
        let mut cache = FormCache::new();
        push_cached(&mut cache.types, CacheKey::new(name("m", "b"), Stack::default(), 3), cached(1));
        push_cached(&mut cache.types, CacheKey::new(name("m", "a"), built.clone(), 2), cached(2));
        push_cached(&mut cache.types, CacheKey::new(name("m", "a"), Stack::default(), 9), cached(3));
        push_cached(&mut cache.types, CacheKey::new(name("m", "a"), parsed, 2), cached(4));
        let entries = cache.entries();
        let order = entries.iter().map(|e| (e.name.name.as_str(), e.enclosing.len(), e.depth, e.cost)).collect::<Vec<_>>();
        assert_eq!(order, [("a", 0, 9, 3), ("a", 2, 2, 2), ("a", 2, 2, 4), ("b", 0, 3, 1)]);
        assert_eq!(entries[1].enclosing, [inner, outer]);
    }
}

#[cfg(test)]
mod record_validation_tests {
    use super::*;
    use crate::forms::{Record, RecordField};
    #[test]
    fn malformed_record_prototype_rejects_atomically() {
        let mut forms = Forms::default();
        forms.records.insert(
            (ModuleId(0), "r".into()),
            Record {
                module: ModuleId(0),
                name: "r".into(),
                fields: vec![RecordField {
                    name: "x".into(),
                    declared_type: None,
                    default_expression: None,
                }],
                metadata: 0,
            },
        );
        let modules = vec!["m".into()];
        let mut resolver = Resolver::new(&forms, &modules);
        resolver
            .set_record_types(BTreeMap::from([(
                (ModuleId(0), "r".into()),
                vec![Type::atom("x")],
            )]))
            .unwrap();
        assert!(
            resolver
                .set_record_types(BTreeMap::from([((ModuleId(0), "r".into()), vec![])]))
                .is_err()
        );
        assert_eq!(
            resolver.record_prototype(ModuleId(0), "r"),
            Some([Type::atom("x")].as_slice())
        );
        assert!(
            resolver
                .set_record_types(BTreeMap::from([((ModuleId(0), "missing".into()), vec![])]))
                .is_err()
        );
    }
}

#[cfg(test)]
mod map_budget_tests {
    use super::*;
    use crate::forms::Form;

    fn fields(names: &[&str]) -> (Forms, Vec<FormId>) {
        let mut forms = Forms::default();
        let mut add = |value| {
            let id = FormId(forms.nodes.len());
            forms.nodes.push(Form {
                module: ModuleId(0),
                raw: crate::core::TermId(id.0),
                value,
            });
            id
        };
        let mut ids = Vec::new();
        for name in names {
            let key = add(TypeForm::Atom((*name).into()));
            let value = add(TypeForm::Atom(format!("value_{name}")));
            ids.push(add(TypeForm::Builtin {
                name: Builtin::MapFieldExact,
                arguments: Some(vec![key, value]),
            }));
        }
        (forms, ids)
    }

    #[test]
    fn required_union_map_key_does_not_require_every_alternative() {
        let (mut forms, ids) = fields(&["a"]);
        let a = FormId(forms.nodes.len());
        forms.nodes.push(Form { module: ModuleId(0), raw: crate::core::TermId(a.0),
            value: TypeForm::Atom("a".into()) });
        let b = FormId(forms.nodes.len());
        forms.nodes.push(Form { module: ModuleId(0), raw: crate::core::TermId(b.0),
            value: TypeForm::Atom("b".into()) });
        forms.nodes[0].value = TypeForm::Builtin { name: Builtin::Union, arguments: Some(vec![a, b]) };
        let modules = vec!["union_map".into()];
        let mut resolver = Resolver::new(&forms, &modules);
        let (actual, _) = resolver.map(&ids, &State::default(), 16, 100).unwrap();
        let expected = Type::map(["a", "b"].into_iter().map(|key| MapEntry {
            key: MapKey::from_type(&Type::atom(key)).unwrap(), presence: Presence::Optional,
            value: Type::atom("value_a"),
        }).collect(), Type::none(), Type::none()).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn exhausted_map_budget_keeps_prefix_including_final_and_empty_tail() {
        for names in [
            vec![],
            vec!["a"],
            vec!["a", "b", "c"],
            vec!["c", "a", "b"],
            vec!["b", "c", "a"],
        ] {
            let (forms, ids) = fields(&names);
            let modules = vec!["map_budget".into()];
            for budget in -1..=5 {
                let mut resolver = Resolver::new(&forms, &modules);
                let (actual, left) = resolver.map(&ids, &State::default(), 16, budget).unwrap();
                let used = usize::try_from(budget.max(0)).unwrap().min(names.len());
                let mut entries: Vec<MapEntry> = names[..used]
                    .iter()
                    .map(|name| MapEntry {
                        key: MapKey::from_type(&Type::atom(*name)).unwrap(),
                        presence: Presence::Mandatory,
                        value: Type::atom(&format!("value_{name}")),
                    })
                    .collect();
                entries.sort_by(|a, b| a.key.cmp(&b.key));
                let exhausted = budget <= names.len() as i64;
                let default = if exhausted { Type::any() } else { Type::none() };
                let expected = Type::map(entries, default.clone(), default).unwrap();
                assert_eq!(actual, expected, "order={names:?}, budget={budget}");
                assert_eq!(left, budget - used as i64);
            }
        }
    }

    #[test]
    fn actual_inspect_opts_forms_resolve_when_requested() {
        let Ok(path) = std::env::var("INSPECT_OPTS_CORE_PROOF") else {
            return;
        };
        let core =
            crate::core::CoreProgram::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
        let program = crate::program::Program::build(&core).unwrap();
        let forms = Forms::build(&core, &program).unwrap();
        let modules: Vec<_> = core.modules.iter().map(|m| m.name.clone()).collect();
        let declaration = forms
            .declarations
            .values()
            .find(|decl| {
                decl.name.name == "t" && decl.name.module == "Elixir.Inspect.Opts"
            })
            .unwrap();
        let mut resolver = Resolver::new(&forms, &modules);
        let typ = resolver
            .resolve(
                declaration.body,
                Site::Type(declaration.name.clone()),
                BTreeMap::new(),
            )
            .unwrap();
        let value = typ.to_value();
        let inspect = value["entries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|entry| entry["key"]["values"] == serde_json::json!(["inspect_fun"]))
            .unwrap();
        let callback_map = &inspect["value"]["domain"][1];
        let keys: Vec<_> = callback_map["entries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["key"]["values"][0].as_str().unwrap())
            .collect();
        assert_eq!(keys, ["__struct__", "base", "binaries"]);
        eprintln!("actual Inspect.Opts recursive callback map retains OTP prefix: {keys:?}");
    }
}

#[cfg(all(test, otp_domain = "29"))]
mod native_record_form_tests {
    use super::*;
    use crate::{core::{ErlangTerm, TermId}, native_form_environment::{NativeFormSite,NativeSiteKind,RawFormTerm}};
    fn ast_atom(b:&mut ReasonBuilder,name:&str)->TermId {let tag=b.atom("atom");let zero=b.integer(0);let name=b.atom(name);b.tuple(vec![tag,zero,name])}
    fn builtin(b:&mut ReasonBuilder,name:&str,args:Vec<TermId>)->TermId {let tag=b.atom("type");let zero=b.integer(0);let name=b.atom(name);let args=b.list(args);b.tuple(vec![tag,zero,name,args])}
    fn qualified(b:&mut ReasonBuilder,module:&str,name:&str)->TermId {let m=ast_atom(b,module);let n=ast_atom(b,name);let tag=b.atom("tuple");let zero=b.integer(0);let pair=b.list(vec![m,n]);b.tuple(vec![tag,zero,pair])}
    fn fixture(qualified_name:bool, recursive:bool)->(RawFormTerm,BTreeMap<String,ModuleTableHandle>,NativeFormSite) {
        let mut b=ReasonBuilder::new();
        let field_form=if recursive {let name=qualified(&mut b,"m","r");builtin(&mut b,"record",vec![name])} else {builtin(&mut b,"integer",vec![])};
        let x=b.atom("x");let cached=b.native(&Type::all_integers().to_native_value());let field=b.tuple(vec![x,field_form,cached]);let fields=b.list(vec![field]);
        let file=b.string("test.erl");let zero=b.integer(0);let location=b.tuple(vec![file,zero]);let value=b.tuple(vec![location,fields]);
        let tag=b.atom("native_record");let m=b.atom("m");let r=b.atom("r");let pair=b.tuple(vec![m,r]);let key=b.tuple(vec![tag,pair]);let table=b.push(ErlangTerm::Map(vec![(key,value)]));
        let tag=b.atom("import_record");let import=b.tuple(vec![tag,r]);let caller=b.push(ErlangTerm::Map(vec![(import,m)]));
        let name=if qualified_name {qualified(&mut b,"m","r")} else {ast_atom(&mut b,"r")};let form=builtin(&mut b,"record",vec![name]);
        let raw=b.finish(form);let a=raw.arena().clone();
        let tables=BTreeMap::from([("m".into(),ModuleTableHandle::at(a.clone(),table).unwrap()),("caller".into(),ModuleTableHandle::at(a.clone(),caller).unwrap())]);
        let site=NativeFormSite{kind:NativeSiteKind::Spec,scope:TypeName{module:"caller".into(),name:"f".into(),arity:0}.into(),file:RawFormTerm::new(a,file).unwrap()};
        (raw,tables,site)
    }
    fn run(raw:&RawFormTerm,tables:BTreeMap<String,ModuleTableHandle>,site:NativeFormSite,check:bool)->Result<Type> {
        let names=vec!["caller".into(),"m".into()];let mut forms=Forms::default();let id=forms.defer_raw(ModuleId(0),raw);
        let mut resolver=Resolver::new(&forms,&names);resolver.set_source_tables(tables);resolver.set_native_site(site);
        if check {resolver.check_record_fields(id,&BTreeMap::new())?;Ok(Type::any())} else {resolver.resolve(id,Site::Spec,BTreeMap::new())}
    }
    #[test]
    fn qualified_and_imported_forms_resolve_the_same_prototype() {
        let (a,tables,site)=fixture(true,false);let qualified=run(&a,tables,site,false).unwrap();
        let (a,tables,site)=fixture(false,false);let imported=run(&a,tables,site,false).unwrap();
        assert_eq!(qualified,imported);assert_eq!(qualified.record_get("x").unwrap(),Type::all_integers());
        assert_eq!(crate::lattice::PresentationContext::default().type_to_string(&qualified).unwrap(),"#m:r{x=integer()}");
    }
    #[test]
    fn unknown_remote_module_keeps_a_named_generic_record() {
        let (a,mut tables,site)=fixture(true,false);tables.remove("m");let value=run(&a,tables,site,false).unwrap();
        assert_eq!(crate::lattice::PresentationContext::default().type_to_string(&value).unwrap(),"#m:r{}");
    }
    #[test]
    fn native_record_recursion_unfolds_twice_then_keeps_identity() {
        let (a,tables,site)=fixture(true,true);let value=run(&a,tables,site,false).unwrap();
        assert_eq!(crate::lattice::PresentationContext::default().type_to_string(&value).unwrap(),"#m:r{x=#m:r{x=#m:r{}}}");
    }
    #[test]
    fn qualified_check_retains_upstream_module_badmatch() {
        let (a,tables,site)=fixture(true,false);
        assert!(matches!(run(&a,tables,site,true),Err(FormError::Native(NativeTransferError::BadMatchNative(_)))));
        let (a,tables,site)=fixture(false,false);run(&a,tables,site,true).unwrap();
    }
    #[test]
    fn builtin_record_has_ninth_family() {
        let mut b=ReasonBuilder::new();let root=builtin(&mut b,"record",vec![]);let raw=b.finish(root);
        let (_,tables,site)=fixture(false,false);assert_eq!(run(&raw,tables,site,false).unwrap(),Type::record_any());
        let mut b=ReasonBuilder::new();let tag=b.atom("type");let zero=b.integer(0);let record=b.atom("record");let any=b.atom("any");
        let root=b.tuple(vec![tag,zero,record,any]);let raw=b.finish(root);
        let (_,tables,site)=fixture(false,false);
        assert!(matches!(run(&raw,tables,site,false),Err(FormError::Native(NativeTransferError::FunctionClause))));
    }
    #[test]
    fn alias_arguments_resolve_records_in_the_callers_module() {
        for remote in [false, true] {
            for native in [false, true] {
                let (record, mut tables, site) = fixture(false, false);
                let mut b = ReasonBuilder::new();
                let argument = b.copy(&record);
                let owner = if remote { "generic" } else { "caller" };
                let module = b.atom(owner);
                let file = b.string("generic.erl");
                let zero = b.integer(0);
                let location = b.tuple(vec![file, zero]);
                let var = b.atom("var");
                let parameter = b.atom("T");
                let body = b.tuple(vec![var, zero, parameter]);
                let parameters = b.list(vec![parameter]);
                let definition = b.tuple(vec![module, location, body, parameters]);
                let any = b.atom("any");
                let definition = b.tuple(vec![definition, any]);
                let type_tag = b.atom("type");
                let identity = b.atom("identity");
                let one = b.integer(1);
                let key = b.tuple(vec![type_tag, identity, one]);
                let mut entries = Vec::new();
                if !remote {
                    let original = tables["caller"].raw();
                    let ErlangTerm::Map(rows) = original.term() else { unreachable!() };
                    for (key, value) in rows {
                        entries.push((b.copy(&RawFormTerm::new(original.arena().clone(), *key).unwrap()),
                            b.copy(&RawFormTerm::new(original.arena().clone(), *value).unwrap())));
                    }
                }
                entries.push((key, definition));
                let table = b.push(ErlangTerm::Map(entries));
                let arguments = b.list(vec![argument]);
                let call = if remote {
                    let tag = b.atom("remote_type");
                    let module = ast_atom(&mut b, owner);
                    let name = ast_atom(&mut b, "identity");
                    let parts = b.list(vec![module, name, arguments]);
                    b.tuple(vec![tag, zero, parts])
                } else {
                    let tag = b.atom("user_type");
                    b.tuple(vec![tag, zero, identity, arguments])
                };
                let raw = b.finish(call);
                tables.insert(owner.into(), ModuleTableHandle::at(raw.arena().clone(), table).unwrap());
                let names = vec!["caller".into(), "m".into(), "generic".into()];
                let mut forms = Forms::default();
                let id = forms.defer_raw(ModuleId(0), &raw);
                if !native {
                    forms.records.insert((ModuleId(0), "r".into()), crate::forms::Record {
                        module: ModuleId(0), name: "r".into(), fields: Vec::new(), metadata: usize::MAX,
                    });
                }
                let mut resolver = Resolver::new(&forms, &names);
                resolver.set_source_tables(tables);
                resolver.set_exported_types(BTreeSet::from([TypeName {module:owner.into(),name:"identity".into(),arity:1}]));
                resolver.set_native_site(site);
                let actual = resolver.resolve(id, Site::Spec, BTreeMap::new())
                    .unwrap_or_else(|error| panic!("remote={remote}, native={native}: {error}"));
                let expected = if native {
                    let (raw, tables, site) = fixture(false, false);
                    run(&raw, tables, site, false).unwrap()
                } else { Type::tuple(vec![Type::atom("r")]).unwrap() };
                assert_eq!(actual, expected, "remote={remote}, native={native}");
            }
        }
    }
}
