use crate::{
    labels::CoreLabel,
    program::{Program, TargetContracts},
    term_order::{CaptureId, CaptureTerm, RuntimeIdentity, RuntimeKind},
};
#[derive(Clone, Debug)]
pub struct SourceReference(pub RuntimeIdentity);
impl SourceReference {
    fn append(&self, terms: &mut Vec<CaptureTerm>) -> CaptureId {
        push(
            terms,
            CaptureTerm::Runtime {
                kind: RuntimeKind::Reference,
                identity: self.0.clone(),
            },
        )
    }
}
#[derive(Clone, Debug)]
pub struct DigraphCapture {
    pub vertices: SourceReference,
    pub edges: SourceReference,
    pub neighbours: SourceReference,
    pub cyclic: bool,
}
#[derive(Clone, Debug)]
pub enum ActiveDigraphCapture {
    Undefined,
    Digraph(DigraphCapture),
    Outgoing(SourceReference),
}
#[derive(Clone, Debug)]
pub struct CallGraphCapture {
    pub digraph: DigraphCapture,
    pub active: ActiveDigraphCapture,
    pub escaping: SourceReference,
    pub letrec: SourceReference,
    pub names: SourceReference,
    pub reverse_names: SourceReference,
    pub recursive_variables: SourceReference,
    pub self_recursive: SourceReference,
    pub calls: SourceReference,
}
#[derive(Clone, Debug)]
pub enum CodeServerTable {
    Clean,
    Live(SourceReference),
}
#[derive(Clone, Debug)]
pub struct CodeServerCapture {
    pub next_core_label: CoreLabel,
    pub code: SourceReference,
    pub exported_types: CodeServerTable,
    pub records: CodeServerTable,
    pub contracts: SourceReference,
    pub callbacks: SourceReference,
    pub function_metadata: SourceReference,
    pub exports: CodeServerTable,
    pub temporary_exported_types: CodeServerTable,
    pub temporary_records: CodeServerTable,
    pub temporary_contracts: CodeServerTable,
    pub temporary_callbacks: CodeServerTable,
}
#[derive(Clone, Debug)]
pub struct PltCapture {
    pub signatures: SourceReference,
    pub types: SourceReference,
    pub contracts: SourceReference,
    pub callbacks: SourceReference,
    pub exported_types: SourceReference,
}
#[derive(Clone, Debug, Default)]
pub struct GenerationRuntimeCaptures {
    pub callgraph: Option<CallGraphCapture>,
    pub codeserver: Option<CodeServerCapture>,
    pub plt: Option<PltCapture>,
}
fn push(terms: &mut Vec<CaptureTerm>, value: CaptureTerm) -> CaptureId {
    let id = CaptureId(terms.len());
    terms.push(value);
    id
}
fn atom(terms: &mut Vec<CaptureTerm>, value: &str) -> CaptureId {
    push(terms, CaptureTerm::Atom(value.into()))
}
fn record(terms: &mut Vec<CaptureTerm>, name: &str, fields: Vec<CaptureId>) -> CaptureId {
    let tag = atom(terms, name);
    let mut values = vec![tag];
    values.extend(fields);
    push(terms, CaptureTerm::Tuple(values))
}
impl DigraphCapture {
    pub fn append(&self, terms: &mut Vec<CaptureTerm>) -> CaptureId {
        let fields = vec![
            self.vertices.append(terms),
            self.edges.append(terms),
            self.neighbours.append(terms),
            atom(terms, if self.cyclic { "true" } else { "false" }),
        ];
        record(terms, "digraph", fields)
    }
}
impl CodeServerTable {
    fn append(&self, terms: &mut Vec<CaptureTerm>) -> CaptureId {
        match self {
            Self::Clean => atom(terms, "clean"),
            Self::Live(reference) => reference.append(terms),
        }
    }
}
impl CallGraphCapture {
    pub fn append(&self, terms: &mut Vec<CaptureTerm>) -> CaptureId {
        let digraph = self.digraph.append(terms);
        let active = match &self.active {
            ActiveDigraphCapture::Undefined => atom(terms, "undefined"),
            ActiveDigraphCapture::Digraph(graph) => {
                let tag = atom(terms, "d");
                let graph = graph.append(terms);
                push(terms, CaptureTerm::Tuple(vec![tag, graph]))
            }
            ActiveDigraphCapture::Outgoing(reference) => {
                let tag = atom(terms, "e");
                let reference = reference.append(terms);
                push(terms, CaptureTerm::Tuple(vec![tag, reference]))
            }
        };
        let fields = vec![
            digraph,
            active,
            self.escaping.append(terms),
            self.letrec.append(terms),
            self.names.append(terms),
            self.reverse_names.append(terms),
            self.recursive_variables.append(terms),
            self.self_recursive.append(terms),
            self.calls.append(terms),
        ];
        record(terms, "callgraph", fields)
    }
}
impl CodeServerCapture {
    pub fn append(&self, terms: &mut Vec<CaptureTerm>) -> CaptureId {
        let next = push(
            terms,
            CaptureTerm::Integer(self.next_core_label.0.to_string()),
        );
        let fields = vec![
            next,
            self.code.append(terms),
            self.exported_types.append(terms),
            self.records.append(terms),
            self.contracts.append(terms),
            self.callbacks.append(terms),
            self.function_metadata.append(terms),
            self.exports.append(terms),
            self.temporary_exported_types.append(terms),
            self.temporary_records.append(terms),
            self.temporary_contracts.append(terms),
            self.temporary_callbacks.append(terms),
        ];
        record(terms, "codeserver", fields)
    }
}
impl PltCapture {
    pub fn append(&self, terms: &mut Vec<CaptureTerm>) -> CaptureId {
        let fields = vec![
            self.signatures.append(terms),
            self.types.append(terms),
            self.contracts.append(terms),
            self.callbacks.append(terms),
            self.exported_types.append(terms),
        ];
        record(terms, "plt", fields)
    }
}
impl GenerationRuntimeCaptures {
    pub fn for_analysis(
        scope: String,
        next_core_label: CoreLabel,
        program: &Program,
    ) -> Result<Self, String> {
        if scope.is_empty() {
            return Err("analysis runtime allocation scope missing".into());
        }
        let reference = |object: &str| {
            SourceReference(RuntimeIdentity {
                scope: scope.clone(),
                object: format!("source_base/{object}"),
            })
        };
        let temporary_table = |object: &str| match program.target_contracts() {
            TargetContracts::Use => CodeServerTable::Clean,
            TargetContracts::Ignore => CodeServerTable::Live(reference(object)),
        };
        Ok(Self {
            callgraph: Some(CallGraphCapture {
                digraph: DigraphCapture {
                    vertices: reference("callgraph/vertices"),
                    edges: reference("callgraph/edges"),
                    neighbours: reference("callgraph/neighbours"),
                    cyclic: true,
                },
                active: ActiveDigraphCapture::Undefined,
                escaping: reference("callgraph/escaping"),
                letrec: reference("callgraph/letrec"),
                names: reference("callgraph/names"),
                reverse_names: reference("callgraph/reverse_names"),
                recursive_variables: reference("callgraph/recursive_variables"),
                self_recursive: reference("callgraph/self_recursive"),
                calls: reference("callgraph/calls"),
            }),
            codeserver: Some(CodeServerCapture {
                next_core_label,
                code: reference("codeserver/code"),
                exported_types: CodeServerTable::Live(reference("codeserver/exported_types")),
                records: CodeServerTable::Live(reference("codeserver/records")),
                contracts: reference("codeserver/contracts"),
                callbacks: reference("codeserver/callbacks"),
                function_metadata: reference("codeserver/function_metadata"),
                exports: CodeServerTable::Live(reference("codeserver/exports")),
                temporary_exported_types: CodeServerTable::Clean,
                temporary_records: temporary_table("codeserver/temporary_records"),
                temporary_contracts: temporary_table("codeserver/temporary_contracts"),
                temporary_callbacks: temporary_table("codeserver/temporary_callbacks"),
            }),
            plt: Some(PltCapture {
                signatures: reference("plt/signatures"),
                types: reference("plt/types"),
                contracts: reference("plt/contracts"),
                callbacks: reference("plt/callbacks"),
                exported_types: reference("plt/exported_types"),
            }),
        })
    }
    pub fn append(&self, terms: &mut Vec<CaptureTerm>) -> [CaptureId; 3] {
        let graph = match &self.callgraph {
            Some(graph) => graph.append(terms),
            None => atom(terms, "undefined"),
        };
        let server = match &self.codeserver {
            Some(server) => server.append(terms),
            None => atom(terms, "undefined"),
        };
        let plt = match &self.plt {
            Some(plt) => plt.append(terms),
            None => atom(terms, "undefined"),
        };
        [graph, server, plt]
    }
}
