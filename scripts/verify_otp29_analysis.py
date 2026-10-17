#!/usr/bin/env python3
"""Compare OTP29 native-record analysis with the public OTP29 Dialyzer API.

No build is performed. --capture-only records the native oracle before a Rust
worker is ready. Outputs, compiled fixtures and PLTs stay in the requested cache.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
FIXTURES = ROOT / "backend29/tests/fixtures/analysis29"
OPTIONS = ["no_copt", "to_core", "binary", "return_errors", "no_inline",
           "strict_record_tests", "dialyzer", "no_spawn_compiler_process"]
ORACLE = r'''
#!/usr/bin/env escript
-mode(compile).
main(["capture", Manifest, Out]) ->
    "29" = erlang:system_info(otp_release),
    application:load(dialyzer),
    {ok,Input} = file:read_file(Manifest),
    Data = json:decode(Input),
    Beams = [binary_to_list(P) || P <- maps:get(<<"beam_files">>, Data)],
    PltFile = filename:join(Out,"native.plt"),
    file:delete(PltFile),
    Warnings = dialyzer:run([{analysis_type,plt_build},{files,Beams},
        {output_plt,PltFile},{check_plt,false},{get_warnings,true},
        {warnings,[no_unknown]},{error_location,column}]),
    ok = file:write_file(filename:join(Out,"native-warnings.etf"),term_to_binary(Warnings)),
    ok = file:write_file(filename:join(Out,"native-warnings.txt"),
        [dialyzer:format_warning(W,[{indent_opt,false},{filename_opt,fullpath}]) || W <- Warnings]),
    Plt = dialyzer_cplt:from_file(PltFile),
    Mfas = lists:sort(lists:append([begin
        {ok,{M,[{exports,Exports}]}} = beam_lib:chunks(Beam,[exports]),
        [{M,F,A} || {F,A} <- Exports,F =/= module_info]
    end || Beam <- Beams])),
    Signatures = [begin
        {value,{Return,Args}} = dialyzer_plt:lookup(Plt,Mfa),
        signature(Mfa,Return,Args)
    end || Mfa <- Mfas],
    dialyzer_plt:delete(Plt),
    ok = file:write_file(filename:join(Out,"native-mfas.etf"),term_to_binary(Mfas)),
    RuntimeModules = [dialyzer,dialyzer_typesig,dialyzer_contracts,dialyzer_dataflow,
        erl_types,erl_bif_types,compile,v3_core,erl_expand_records,dialyzer_clean_core],
    Runtime = #{otp => unicode:characters_to_binary(erlang:system_info(otp_release)),
        erts => unicode:characters_to_binary(erlang:system_info(version)),
        dialyzer => unicode:characters_to_binary(element(2,application:get_key(dialyzer,vsn))),
        beam_files => maps:from_list([{M,unicode:characters_to_binary(code:which(M))} || M <- RuntimeModules])},
    ok = file:write_file(filename:join(Out,"native.json"),json:encode(#{
        runtime => Runtime,warnings => length(Warnings),signatures => Signatures}));
main(["compare", RustFile, Out]) ->
    {ok,Input} = file:read_file(RustFile), Rust = json:decode(Input),
    <<"ok">> = maps:get(<<"status">>,Rust),
    {ok,WarningsBin} = file:read_file(filename:join(Out,"native-warnings.etf")),
    NativeWarnings = binary_to_term(WarningsBin),
    ActualWarnings = arena(maps:get(<<"warnings">>,Rust)),
    {ok,MfasBin} = file:read_file(filename:join(Out,"native-mfas.etf")),
    Mfas = binary_to_term(MfasBin),
    Plt = dialyzer_cplt:from_file(filename:join(Out,"native.plt")),
    Rows = maps:get(<<"signatures">>,Rust),
    Results = [begin
        Matching = [R || R <- Rows, identity(R) =:= Mfa],
        case Matching of
            [R] ->
                {value,{NativeReturn,NativeArgs}} = dialyzer_plt:lookup(Plt,Mfa),
                ActualReturn = type(maps:get(<<"result">>,R)),
                ActualArgs = [type(A) || A <- maps:get(<<"arguments">>,R)],
                (signature(Mfa,NativeReturn,NativeArgs))#{
                    matches => NativeReturn =:= ActualReturn andalso NativeArgs =:= ActualArgs,
                    actual_result => text(ActualReturn), actual_arguments => [text(A) || A <- ActualArgs]};
            _ -> #{identity => mfa(Mfa), matches => false, rust_rows => length(Matching)}
        end
    end || Mfa <- Mfas],
    dialyzer_plt:delete(Plt),
    Report = #{warnings_exact => NativeWarnings =:= ActualWarnings,
        native_warning_count => length(NativeWarnings), rust_warning_count => length(ActualWarnings),
        signatures_exact => lists:all(fun(R) -> maps:get(matches,R) end,Results),
        exported_signatures => length(Mfas), signatures => Results,
        native_warnings => unicode:characters_to_binary(io_lib:format("~tp",[NativeWarnings])),
        rust_warnings => unicode:characters_to_binary(io_lib:format("~tp",[ActualWarnings]))},
    ok = file:write_file(filename:join(Out,"comparison.json"),json:encode(Report)).
identity(R) -> {binary_to_atom(maps:get(<<"module">>,R)),binary_to_atom(maps:get(<<"function">>,R)),maps:get(<<"arity">>,R)}.
mfa({M,F,A}) -> [atom_to_binary(M),atom_to_binary(F),A].
text(T) -> unicode:characters_to_binary(erl_types:t_to_string(T)).
signature(Mfa,Return,Args) -> #{identity => mfa(Mfa),result => text(Return),arguments => [text(A) || A <- Args]}.
integer(<<"neg_inf">>) -> neg_inf;
integer(<<"pos_inf">>) -> pos_inf;
integer(N) when is_binary(N) -> binary_to_integer(N);
integer(N) when is_integer(N) -> N.
record_name(null) -> nil;
record_name(N) when is_binary(N) -> binary_to_atom(N);
record_name(N) -> {binary_to_atom(maps:get(<<"module">>,N)),binary_to_atom(maps:get(<<"name">>,N))}.
nominal_name(#{<<"symbol">> := N}) -> binary_to_atom(N);
nominal_name(#{<<"module">> := M,<<"name">> := N,<<"arity">> := A}) -> {binary_to_atom(M),binary_to_atom(N),A}.
type(#{<<"kind">> := Kind}=T) ->
    case Kind of
        <<"any">> -> erl_types:t_any();
        <<"none">> -> erl_types:t_none();
        <<"unit">> -> erl_types:t_unit();
        <<"atom">> -> erl_types:t_atom();
        <<"atoms">> -> erl_types:t_atoms([binary_to_atom(A) || A <- maps:get(<<"values">>,T)]);
        <<"integer">> -> erl_types:t_integer();
        <<"integers">> -> erl_types:t_integers([integer(N) || N <- maps:get(<<"values">>,T)]);
        <<"range">> -> erl_types:t_from_range(integer(maps:get(<<"from">>,T)),integer(maps:get(<<"to">>,T)));
        <<"float">> -> erl_types:t_float();
        <<"number">> -> erl_types:t_number();
        <<"nil">> -> erl_types:t_nil();
        <<"bitstring">> -> erl_types:t_bitstr(integer(maps:get(<<"unit">>,T)),integer(maps:get(<<"base">>,T)));
        <<"list">> ->
            E = type(maps:get(<<"elements">>,T)), Tail = type(maps:get(<<"tail">>,T)),
            Size = case maps:get(<<"nonempty">>,T) of true -> nonempty; false -> unknown end,
            {c,list,{E,Tail},Size};
        <<"tuple_any">> -> erl_types:t_tuple();
        <<"tuple">> -> erl_types:t_tuple([type(E) || E <- maps:get(<<"elements">>,T)]);
        <<"tuple_set">> -> erl_types:t_sup([type(E) || E <- maps:get(<<"values">>,T)]);
        <<"union">> -> erl_types:t_sup([type(E) || E <- maps:get(<<"values">>,T)]);
        <<"union_raw">> -> {c,union,[type(E) || E <- maps:get(<<"values">>,T)],unknown};
        <<"product">> -> erl_types:t_product([type(E) || E <- maps:get(<<"values">>,T)]);
        <<"product_raw">> -> {c,product,[type(E) || E <- maps:get(<<"values">>,T)],unknown};
        <<"record">> -> {c,record,{record_name(maps:get(<<"name">>,T)),maps:from_list([
            {binary_to_atom(maps:get(<<"key">>,F)),case maps:get(<<"presence">>,F) of
                <<"missing">> -> missing; <<"present">> -> {present,type(maps:get(<<"value">>,F))}
            end} || F <- maps:get(<<"fields">>,T)])},unknown};
        <<"record_set">> -> {c,record_set,[type(E) || E <- maps:get(<<"values">>,T)],unknown};
        <<"map">> -> erl_types:t_map([{type(maps:get(<<"key">>,E)),binary_to_atom(maps:get(<<"presence">>,E)),type(maps:get(<<"value">>,E))} || E <- maps:get(<<"entries">>,T)], type(maps:get(<<"default_key">>,T)),type(maps:get(<<"default_value">>,T)));
        <<"map_any">> -> erl_types:t_map();
        <<"function">> ->
            Range = type(maps:get(<<"range">>,T)),
            case maps:get(<<"domain">>,T) of null -> erl_types:t_fun(Range); Args -> erl_types:t_fun([type(A) || A <- Args],Range) end;
        <<"identifier">> -> erl_types:t_identifier();
        <<"identifiers">> -> erl_types:t_sup([case I of <<"pid">> -> erl_types:t_pid(); <<"port">> -> erl_types:t_port(); <<"reference">> -> erl_types:t_reference() end || I <- maps:get(<<"values">>,T)]);
        <<"nominal">> -> erl_types:t_nominal(nominal_name(maps:get(<<"name">>,T)),type(maps:get(<<"inner">>,T)));
        <<"nominal_raw">> -> {c,nominal,{nominal_name(maps:get(<<"name">>,T)),type(maps:get(<<"inner">>,T))},unknown};
        <<"nominal_set">> -> {c,nominal_set,{[type(E) || E <- maps:get(<<"nominals">>,T)],type(maps:get(<<"structural">>,T))},unknown};
        _ -> erlang:error({unsupported_exact_type,Kind,T})
    end.
arena(#{<<"root">> := Root,<<"terms">> := Terms}) ->
    {Values,_} = lists:foldl(fun(T,{Acc,I}) -> {Acc#{I => term(T,Acc)},I+1} end,{#{},0},Terms),
    maps:get(Root,Values).
term(#{<<"kind">> := <<"atom">>,<<"value">> := A},_) -> binary_to_atom(A);
term(#{<<"kind">> := <<"integer">>,<<"value">> := N},_) -> integer(N);
term(#{<<"kind">> := <<"nil">>},_) -> [];
term(#{<<"kind">> := <<"tuple">>,<<"items">> := Is},V) -> list_to_tuple([maps:get(I,V) || I <- Is]);
term(#{<<"kind">> := <<"list">>,<<"items">> := Is,<<"tail">> := Tail},V) -> lists:foldr(fun(I,T) -> [maps:get(I,V)|T] end,maps:get(Tail,V),Is);
term(#{<<"kind">> := <<"map">>,<<"entries">> := Pairs},V) -> maps:from_list([{maps:get(K,V),maps:get(X,V)} || [K,X] <- Pairs]);
term(#{<<"kind">> := <<"binary">>,<<"value">> := B},_) -> base64:decode(B);
term(T,_) -> erlang:error({unsupported_exact_warning_term,T}).
'''.lstrip()


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def execute(command, out, name, timeout=120, allow_failure=False):
    result = subprocess.run(list(map(str, command)), cwd=ROOT, capture_output=True,
                            timeout=timeout)
    (out / (name + ".stdout")).write_bytes(result.stdout)
    (out / (name + ".stderr")).write_bytes(result.stderr)
    if result.returncode and not allow_failure:
        raise RuntimeError(f"{name} exited {result.returncode}: {result.stderr.decode()[-3000:]}")
    return result


def empty_prior(profile):
    nil = {"root": 0, "terms": [{"kind": "nil"}]}
    return {"version": 5, "profile": profile, "modules": [],
            "source": {"kind": "classic_plt", "path": "", "sha256": "", "files": []},
            "contracts": [], "summaries": [], "raw_callback_groups": nil,
            "raw_summaries": nil, "raw_contracts": nil, "exported_types": nil,
            "type_tables": {"root": 0, "terms": [{"kind": "map", "entries": [], "iterator_order": []}]}}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--runtime", type=Path, default=ROOT / ".cache/otp-29.1.2/runtime")
    parser.add_argument("--probe", type=Path, default=ROOT / "target/release/core_cache_analyze_otp29")
    parser.add_argument("--out", type=Path, required=True)
    parser.add_argument("--capture-only", action="store_true")
    parser.add_argument("--reuse-oracle", action="store_true")
    args = parser.parse_args()
    runtime, probe, out = args.runtime.resolve(), args.probe.resolve(), args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    escript = runtime / "bin/escript"
    oracle = out / "oracle.escript"
    oracle.write_text(ORACLE)
    warnings = [tag for tag in json.loads((ROOT / "corpus/warnings.json").read_text())["default"] if tag != "warn_unknown"]
    groups = {"records": ["record_dep29", "record_flow29"], "comprehensions": ["comprehension_flow29"]}
    source_paths = sorted(FIXTURES.glob("*.erl")) + [Path(__file__).resolve()]
    before = {str(path): sha(path) for path in source_paths}
    results = []
    for name, modules in groups.items():
        case = out / name
        case.mkdir(exist_ok=True)
        manifest_file = case / "manifest.json"
        if not args.reuse_oracle:
            for module in modules:
                source = FIXTURES / (module + ".erl")
                execute([runtime / "bin/erlc", "+debug_info", "-o", case, source], case, module + "-compile")
            manifest = {"beam_files": [str(case / (module + ".beam")) for module in modules]}
            manifest_file.write_text(json.dumps(manifest, indent=2) + "\n")
            execute([escript, oracle, "capture", manifest_file, case], case, "native-capture", timeout=180)
        native = json.loads((case / "native.json").read_text())
        version = native["runtime"]
        if version["otp"] != "29":
            raise RuntimeError("oracle runtime must be OTP29")
        runtime_beams = version["beam_files"]
        manifest = json.loads(manifest_file.read_text())
        manifest.update(runtime_context="native", dialyzer_typesig_beam=runtime_beams["dialyzer_typesig"], dialyzer_contracts_beam=runtime_beams["dialyzer_contracts"])
        manifest_file.write_text(json.dumps(manifest, indent=2) + "\n")
        provenance_paths = [Path(p) for p in manifest["beam_files"] + list(runtime_beams.values())]
        provenance_paths += [escript, runtime / "bin/erl", runtime / "bin/erlc"]
        provenance = {str(p): sha(p) for p in provenance_paths}
        provenance_file = case / "provenance.json"
        if args.reuse_oracle:
            if json.loads(provenance_file.read_text()) != provenance:
                raise RuntimeError("captured oracle BEAM/runtime inputs changed")
        else:
            provenance_file.write_text(json.dumps(provenance, indent=2) + "\n")
        if args.capture_only:
            results.append({"id": name, "captured": True, "warnings": native["warnings"], "exported_signatures": len(native["signatures"])})
            continue
        profile = {key: version[key] for key in ("otp", "erts", "dialyzer")}
        profile.update(elixir="1.19.4", term_map_order="native_to_list_with_iterator", core_options=OPTIONS)
        prior = case / "empty-prior.json"
        prior.write_text(json.dumps(empty_prior(profile)) + "\n")
        candidate_hash = sha(probe)
        projected = {}
        failed = False
        for phase, mode, flags in [("signatures", "plt_build", []), ("warnings", "succ_typings", ["--warnings-only"])]:
            options = case / ("options-" + phase + ".json")
            options.write_text(json.dumps({"id": name, "analysis_mode": mode, "purged_modules": modules, "legal_warnings": warnings}) + "\n")
            result = execute([probe, prior, manifest_file, options, *flags], case, "rust-" + phase, timeout=180, allow_failure=True)
            actual = json.loads(result.stdout)
            (case / ("rust-" + phase + ".json")).write_bytes(result.stdout)
            if actual.get("status") != "ok":
                results.append({"id": name, "phase": phase, "status": actual.get("status"), "stage": actual.get("stage"), "detail": actual.get("detail")})
                failed = True
                break
            projected[phase] = actual
        if failed:
            continue
        combined = dict(projected["warnings"], signatures=projected["signatures"]["signatures"])
        rust = case / "rust.json"
        rust.write_text(json.dumps(combined) + "\n")
        execute([escript, oracle, "compare", rust, case], case, "compare")
        comparison = json.loads((case / "comparison.json").read_text())
        if sha(probe) != candidate_hash:
            raise RuntimeError("Rust executable changed during comparison")
        if any(sha(Path(p)) != digest for p, digest in provenance.items()):
            raise RuntimeError("BEAM or runtime changed during comparison")
        results.append({"id": name, **comparison, "probe_sha256": candidate_hash})
        print(json.dumps({k: v for k, v in results[-1].items() if k not in ("signatures", "native_warnings", "rust_warnings")}), flush=True)
    if any(sha(Path(p)) != digest for p, digest in before.items()):
        raise RuntimeError("proof sources changed during run")
    report = {"cases": results, "source_sha256": before,
              "scope": {"public_native_dialyzer": True, "native_frontend": True,
                        "raw_public_warning_order": True, "exact_exported_signature_terms": True,
                        "native_target_facts_as_rust_inputs": False, "empty_prior": True, "rust_signature_mode": "plt_build",
                        "rust_warning_mode": "succ_typings",
                        "full_otp29_support_claim": False}}
    (out / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    if not args.capture_only and not all(row.get("warnings_exact") and row.get("signatures_exact") for row in results):
        raise SystemExit(1)
    print(json.dumps({"cases": len(results), "capture_only": args.capture_only}), flush=True)


if __name__ == "__main__":
    main()
