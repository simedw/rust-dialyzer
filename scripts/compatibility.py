#!/usr/bin/env python3
import hashlib
import json
from pathlib import Path
import sys
from proof_inputs import directory_entries

ROOT = Path(__file__).resolve().parents[1]


def read(path):
    return json.loads(path.read_text()) if path.exists() else {}


def fresh(report):
    hashes = report.get("implementation_sha256", {})
    cases = report.get("cases", [])
    case_records = cases if isinstance(cases, list) else []
    return (
        bool(hashes)
        and all(Path(path).is_dir() and directory_entries(path) == entries for path, entries in report.get("directory_inputs", {}).items())
        and all(
            (ROOT / path).exists()
            and hashlib.sha256((ROOT / path).read_bytes()).hexdigest() == digest
            for path, digest in hashes.items()
        )
        and all(
            (ROOT / path).exists()
            and hashlib.sha256((ROOT / path).read_bytes()).hexdigest() == digest
            for path, digest in report.get("inputs_sha256", {}).items()
        )
        and all(
            Path(case["core_json"]).exists()
            and hashlib.sha256(Path(case["core_json"]).read_bytes()).hexdigest()
            == case["sha256"]
            for case in case_records
            if "core_json" in case
        )
    )


def complete_cases(report, expected):
    ids = [case["id"] for case in report.get("cases", [])]
    return (
        report.get("complete") is True
        and fresh(report)
        and set(ids) == expected
        and len(ids) == len(expected)
    )


def main():
    inventory = read(ROOT / "corpus/inventory.json")
    semantic = read(ROOT / ".cache/corpus/report.json")
    frontend = read(ROOT / ".cache/core/rust-report.json")
    indexed = read(ROOT / ".cache/core/program-report.json")
    forms = read(ROOT / ".cache/core/forms-report.json")
    flow = read(ROOT / ".cache/flow/verification.json")
    algebra = read(ROOT / ".cache/types/report.json")
    bifs = read(ROOT / ".cache/bifs/report.json")
    properties = read(ROOT / ".cache/properties/report.json")
    type_suite = read(ROOT / ".cache/type_suite/report.json")
    closure = read(ROOT / ".cache/closure_dependencies/report.json")
    invalidation = read(ROOT / ".cache/invalidation/report.json")
    incremental = read(ROOT / ".cache/incremental_plan/report.json")
    consistency = read(ROOT / ".cache/consistency/report.json")
    function_state = read(ROOT / ".cache/function_state/report.json")
    source_callgraph = read(ROOT / ".cache/source_callgraph/report.json")
    generation = read(ROOT / ".cache/generation/report.json")
    prepared = read(ROOT / ".cache/preparation/corpus/report.json")
    core_capture = read(ROOT / ".cache/core_capture/report.json")
    body_flow = read(ROOT / ".cache/body_flow/report.json")
    folded = read(ROOT / ".cache/folded_core/report.json")
    underapprox = read(ROOT / ".cache/underapprox/report.json")
    allocation = read(ROOT / ".cache/underapprox_allocations/report.json")
    guard_producer = read(ROOT / ".cache/guard_producer/report.json")
    contract_maps = read(ROOT / ".cache/contract_maps/report.json")
    raw_operand = read(ROOT / ".cache/raw_operand/report.json")
    deferred = read(ROOT / ".cache/deferred_operand/report.json")
    constraints = read(ROOT / ".cache/constraints/report.json")
    captures = read(ROOT / ".cache/captures/report.json")
    patterns = read(ROOT / ".cache/patterns/report.json")
    phase_comparison = read(ROOT / ".cache/success_phase/report.json")
    form_exports = read(ROOT / ".cache/form_exports/report.json")
    form_context = read(ROOT / ".cache/form_context/report.json")
    literal_types = read(ROOT / ".cache/literal_types/report.json")
    constructors = read(ROOT / ".cache/constructors/report.json")
    external_term = read(ROOT / ".cache/external_term/report.json")
    fingerprint = read(ROOT / ".cache/fingerprint/report.json")
    snapshot_file = read(ROOT / ".cache/snapshot_file/report.json")
    fresh_nodes = read(ROOT / ".cache/dataflow_nodes/report.json")
    calls = read(ROOT / ".cache/calls/report.json")
    emission = read(ROOT / ".cache/warning_emission/report.json")
    warning_collection = read(ROOT / ".cache/warning_collection/report.json")
    warning_cache = read(ROOT / ".cache/warning_cache/report.json")
    labels = read(ROOT / ".cache/labels/report.json")
    dataflow_map = read(ROOT / ".cache/dataflow_map/report.json")
    term_order = read(ROOT / ".cache/term_order/report.json")
    rendering = read(ROOT / ".cache/warning_render/report.json")
    callbacks = read(ROOT / ".cache/callbacks/report.json")
    snapshot_check = read(ROOT / ".cache/snapshot_check/report.json")
    activation = read(ROOT / ".cache/contract_book/report.json")
    expected = {case["id"] for case in inventory["semantic_cases"]}
    actual = [case["id"] for case in semantic.get("cases", [])]
    frontend_cases = [case["id"] for case in frontend.get("cases", [])]
    passed_semantic = sum(
        case["status"] == "parity" for case in semantic.get("cases", [])
    )
    gates = [
        {
            "id": "labeled_core_capture",
            "passed": fresh(core_capture) and core_capture.get("complete_captured_core_labels") is True and core_capture.get("compiler_groups") == 327 and core_capture.get("node_occurrences") == 1045056,
            "scope": "Exact compiler-derived labeled Cerl values, including untouched and reannotated occurrences",
        },
        {
            "id": "structural_body_flow",
            "passed": fresh(body_flow) and body_flow.get("complete_captured_structural_body_flow") is True,
            "scope": "Native body traversal helper with explicit phase fixtures; whole-module inference has a separate gate",
        },
        {
            "id": "prepared_compiler_definitions",
            "passed": fresh(prepared) and prepared.get("matches") == 327 and prepared.get("records") == 1849 and prepared.get("contracts") == 1486 and prepared.get("scope", {}).get("exact_processed_contract_capture") is True,
            "scope": "compiler-valid records, candidate contracts/callbacks, final type tables and unknown-type events; malformed reached contexts remain open",
        },
        {
            "id": "shared_literal_folding",
            "passed": fresh(folded) and folded.get("complete_captured_literal_folding") is True and folded.get("gates", {}).get("corpus", {}).get("outcomes") == 75391,
            "scope": "literal producer view retains original analysis identities; full guard traversal remains open",
        },
        {
            "id": "source_clause_underapproximation",
            "passed": fresh(underapprox) and underapprox.get("complete_source_underapprox") is True and underapprox.get("matched_clauses") == 59189,
            "scope": "shared safe underapproximation across every corpus clause; complete clause traversal remains open",
        },
        {
            "id": "source_synthetic_allocation_order",
            "passed": fresh(allocation) and allocation.get("complete_captured_refold_allocation_order") is True and allocation.get("native_refold_allocations") == 276,
            "scope": "Rust derives refold allocation events; native per-event handles never seed analysis",
        },
        {
            "id": "guard_argument_production",
            "passed": fresh(guard_producer) and guard_producer.get("complete_captured_guard_argument_producer") is True and guard_producer.get("outcomes") == 71740,
            "scope": "actual native producer text/errors and evaluation order; complete guard traversal remains open",
        },
        {
            "id": "compiler_contract_variable_maps",
            "passed": fresh(contract_maps) and contract_maps.get("complete_compiler_variable_map_transport") is True and contract_maps.get("outcomes") == 216,
            "scope": "compiler-owned map layout in four fresh VMs; inferred constraint values remain Rust-owned",
        },
        {
            "id": "shared_module_function_state",
            "passed": fresh(function_state) and function_state.get("complete_function_state") is True and function_state.get("complete_source_graph_initialization") is True and function_state.get("functions") == 13748 and function_state.get("state_transitions") == 72891,
            "scope": "dictionary/FIFO/captured environments and state transitions; whole body traversal remains open",
        },
        {
            "id": "ordered_source_callgraph",
            "passed": fresh(source_callgraph) and source_callgraph.get("complete_ordered_source_edges") is True and source_callgraph.get("cases") == 327 and source_callgraph.get("modules") == 571,
            "scope": "ordered edges, confirmed vertices, escaping, self recursion and local incoming callers; native SCC order remains open",
        },
        {
            "id": "core_constraint_generation",
            "passed": fresh(generation) and generation.get("full_core_generation") is True,
            "scope": "shared Core generator; structural expressions and call prototypes proven, complete control flow required",
        },
        {
            "id": "captured_raw_operand_subsets",
            "passed": fresh(raw_operand) and raw_operand.get("complete_captured_raw_operand_subsets") is True,
            "scope": "106332 exact captured scalar/function/product/list outcomes; arbitrary raw families remain open",
        },

        {
            "id": "captured_deferred_form_boundary",
            "passed": fresh(deferred) and deferred.get("complete_captured_deferred_context") is True,
            "scope": "572 exact cache/budget/record contexts; malformed raw type-family algebra remains open",
        },
        {
            "id": "captured_runtime_order",
            "passed": fresh(captures) and captures.get("complete_captured_runtime_order") is True and captures.get("outcomes") == 26462,
            "scope": "flat captures with known closure metadata and shared runtime identities; ordering distinct opaque handles remains open",
        },
        {
            "id": "captured_pattern_binding_and_core_producer",
            "passed": fresh(patterns) and patterns.get("complete_captured_pattern_binding") is True and patterns.get("complete_captured_core_producer") is True,
            "scope": "all 327 groups / 571 modules in broad entry state plus exact context, synthetic and long-string probes; full module lifecycle remains open",
        },
        {
            "id": "typed_constructor_boundary",
            "passed": fresh(constructors) and constructors.get("complete_typed_constructor_boundary") is True,
            "scope": "range endpoints and integer bitstring constructor records; arbitrary inputs and raw-record algebra remain open",
        },
        {
            "id": "finite_etf_transport",
            "passed": fresh(external_term) and external_term.get("complete_finite_etf_transport") is True,
            "scope": "finite terms and malformed byte statuses; runtime identities remain open",
        },
        {
            "id": "compiler_module_fingerprints",
            "passed": fresh(fingerprint) and fingerprint.get("complete_compiler_form_fingerprints") is True,
            "scope": "actual compiler form fingerprints across 571 corpus modules and 32 compiled map attributes; production snapshot orchestration remains open",
        },
        {
            "id": "snapshot_file_transport",
            "passed": fresh(snapshot_file) and snapshot_file.get("complete_finite_snapshot_file_transport") is True,
            "scope": "classic/IPLT records and actual native table reloads; production analysis lifecycle remains open",
        },
        {
            "id": "native_fresh_node_traversal",
            "passed": fresh(fresh_nodes) and fresh_nodes.get("complete_native_fresh_traversal") is True,
            "scope": "all 327 compiler groups; full pattern binding remains open",
        },
        {
            "id": "form_cache_foundation",
            "passed": fresh(form_exports) and form_exports.get("complete_form_cache_foundation") is True,
            "scope": "metadata and original-table cache replay; full exported exception/context boundary remains open",
        },
        {
            "id": "captured_form_context_boundary",
            "passed": fresh(form_context) and form_context.get("complete_captured_context_boundary") is True,
            "scope": "captured exact errors, ordered events and same-VM io_lib iolists; arbitrary parser timing and grammar remain open",
        },
        {
            "id": "finite_literal_types",
            "passed": fresh(literal_types) and literal_types.get("complete_finite_literal_adapter") is True,
            "scope": "finite Core literals; deep type construction and runtime identities remain separate boundaries",
        },
        {
            "id": "original_type_suite",
            "passed": fresh(type_suite) and type_suite.get("complete_original_type_tests") is True,
            "required_cases": 3,
        },
        {
            "id": "closure_dependencies",
            "passed": fresh(closure) and closure.get("complete_closure_dependencies") is True and closure.get("requested") == len(expected) and {c["id"] for c in closure.get("cases", [])} == expected,
            "required_cases": len(expected),
        },
        {
            "id": "incremental_dependency_repair",
            "passed": fresh(invalidation) and invalidation.get("complete_invalidation") is True,
            "scope": "pure dependency expansion and repair; persistence and incremental analysis remain open",
        },
        {
            "id": "incremental_analysis_decisions",
            "passed": fresh(incremental) and incremental.get("complete_incremental_plan") is True,
            "scope": "native orchestration decisions after fingerprint checks; persistence remains open",
        },
        {
            "id": "atomic_and_scoped_constraints",
            "passed": fresh(constraints) and constraints.get("complete_atomic_and_scoped_constraints") is True,
            "scope": "atomic/scoped constraints, references, recursive SCCs, derived operations, normalization and preparation; phase captures, Core generation and production orchestration remain open",
        },
        {
            "id": "success_phase_comparison",
            "passed": fresh(phase_comparison) and phase_comparison.get("complete_phase_comparison") is True,
            "scope": "native SCC/module fixpoint comparison and persisted signature limits; production orchestration remains open",
        },
        {
            "id": "call_transfer_algebra",
            "passed": fresh(calls) and calls.get("complete_call_algebra") is True,
            "scope": "canonical call conjunctions and candidate consensus; production phase migration remains open",
        },
        {
            "id": "warning_emission_rules",
            "passed": fresh(emission) and emission.get("complete_emission_rules") is True,
            "scope": "native reachability, no-return, comparison, suppression and collection rules; Core collector remains open",
        },
        {
            "id": "contract_warning_postprocessing",
            "passed": fresh(warning_collection) and warning_collection.get("complete_contract_warning_postprocessing") is True,
            "scope": "native contract-range origin rewriting and same-origin contract warning filtering; Core warning production remains open",
        },
        {
            "id": "snapshot_classification",
            "passed": fresh(snapshot_check) and snapshot_check.get("complete_snapshot_classification") is True,
            "scope": "parsed snapshot profile/fingerprint/warning checks; disk reading and compiler fingerprint extraction remain open",
        },
        {
            "id": "stored_warning_algebra",
            "passed": fresh(warning_cache) and warning_cache.get("complete_warning_storage_algebra") is True,
            "scope": "stored warning merge/selection and dependency merge; disk lifecycle remains open",
        },
        {
            "id": "callback_type_checks",
            "passed": fresh(callbacks) and callbacks.get("complete_callback_type_checks") is True,
            "scope": "required/optional callback availability, exports and inferred/spec type conformance; metadata/origins/production integration remain open",
        },
        {
            "id": "target_contract_activation",
            "passed": fresh(activation) and activation.get("complete_contract_activation") is True,
            "scope": "native activation and repeated activation; production phase orchestration remains open",
        },
        {
            "id": "shared_core_labels",
            "passed": fresh(labels) and labels.get("complete_core_labels") is True,
            "required_cases": len(expected),
        },
        {
            "id": "dataflow_environment_algebra",
            "passed": fresh(dataflow_map) and dataflow_map.get("complete_environment_algebra") is True,
            "scope": "typed native labels, substitutions and scoped joins; Core adapter and pattern binder remain open",
        },
        {
            "id": "finite_core_term_order",
            "passed": fresh(term_order) and term_order.get("complete_finite_core_term_order") is True,
        },
        {
            "id": "captured_warning_rendering",
            "passed": fresh(rendering) and rendering.get("complete_unindented_render") is True and rendering.get("complete_default_render") is True and rendering.get("complete_raw_term_ordering") is True,
            "scope": "captured/source-derived terms with default layout and raw ordering; production diagnostics and general syntax remain open",
        },
        {
            "id": "compiler_interchange",
            "passed": complete_cases(frontend, expected),
            "passed_cases": len(frontend_cases),
            "required_cases": len(expected),
        },
        {
            "id": "semantic_program_index",
            "passed": complete_cases(indexed, expected),
            "required_cases": len(expected),
        },
        {
            "id": "declaration_grammar",
            "passed": complete_cases(forms, expected),
            "required_cases": len(expected),
        },
        {
            "id": "scalar_inference_checkpoint",
            "passed": fresh(flow)
            and len(flow.get("cases", [])) == 5
            and not flow.get("failures"),
            "scope": "four scalar modules and one structural module in bare target PLTs; partial analyzer coverage",
        },
        {
            "id": "semantic_diagnostics",
            "passed": set(actual) == expected
            and len(actual) == len(expected)
            and passed_semantic == len(expected),
            "passed_cases": passed_semantic,
            "required_cases": len(expected),
        },
        {
            "id": "diagnostic_details_and_signatures",
            "passed": semantic.get("details_and_signatures_verified") is True,
            "reason": "Normalized category/location parity alone does not verify details or inferred signatures",
        },
        {
            "id": "complete_type_algebra",
            "passed": fresh(algebra)
            and algebra.get("complete_domain") is True
            and not algebra.get("unsupported_families")
            and not algebra.get("unsupported_operations")
            and not algebra.get("failures"),
            "native_comparisons_passed": algebra.get("passed", 0),
            "unsupported_families": algebra.get("unsupported_families", []),
            "unsupported_operations": algebra.get("unsupported_operations", []),
        },
        {
            "id": "builtin_transfers",
            "passed": fresh(bifs) and bifs.get("differential_complete") is True,
            "required_mfas": read(ROOT / "corpus/bifs.json")
            .get("counts", {})
            .get("mfas", 0),
        },
        {
            "id": "operational_and_type_cases",
            "passed": all(
                suite["status"] == "verified"
                for suite in inventory["otp_handwritten_suites"]
            ),
            "required_cases": sum(
                len(suite["cases"]) for suite in inventory["otp_handwritten_suites"]
            ),
        },
        {
            "id": "generated_consistency",
            "passed": fresh(consistency) and consistency.get("complete_generated_consistency") is True
            and set(consistency.get("verified_suites", [])) == {case["suite"] for case in inventory["otp_consistency_checks"]}
            and consistency.get("matched") == 2 * len(inventory["otp_consistency_checks"]),
            "required_cases": len(inventory["otp_consistency_checks"]),
        },
        {
            "id": "full_domain_properties",
            "passed": fresh(properties)
            and properties.get("full_domain_properties_verified") is True,
            "required_properties": [
                "absorption",
                "associativity",
                "commutativity",
                "idempotence",
                "identity",
                "limit",
            ],
        },
    ]
    report = {
        "version": 1,
        "complete": all(gate["passed"] for gate in gates),
        "gates": gates,
    }
    output = ROOT / ".cache/compatibility/report.json"
    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(json.dumps(report, indent=2) + "\n")
    for gate in gates:
        print(f"{'PASS' if gate['passed'] else 'OPEN'}: {gate['id']}")
    sys.exit(0 if report["complete"] else 1)


if __name__ == "__main__":
    main()
