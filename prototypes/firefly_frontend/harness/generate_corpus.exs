# Oracle-only corpus capture. Existing BEAM files are not recompiled.
[root, out | options] = System.argv()
Code.require_file("core_export.exs", Path.join(root, "scripts"))
destination = Path.join(out, "corpus")
File.mkdir_p!(destination)
base = Path.join(root, ".cache/corpus/otp/base.plt")
prior = Path.join(out,"prior-library.json") |> File.read!() |> :json.decode()
base_sha = Base.encode16(:crypto.hash(:sha256,File.read!(base)),case: :lower)
true = prior["source"]["sha256"] == base_sha
contracts = Path.join(out,"contract-profile.json") |> File.read!() |> :json.decode()
tags = Path.join(root,"corpus/warnings.json") |> File.read!() |> :json.decode() |> Map.fetch!("default") |> Enum.reject(&(&1 == "warn_unknown"))
paths = if "--bundled-fixtures" in options do
  beams = Path.join(out,"corpus-beams")
  File.mkdir_p!(beams)
  for source <- Path.wildcard(Path.join(out,"fixtures/*.erl")) do
    {:ok, module} = :compile.file(String.to_charlist(source), [:debug_info, :return_errors, {:outdir, String.to_charlist(beams)}])
    Path.join(beams, Atom.to_string(module) <> ".beam")
  end
else
  [Path.join(root,".cache/flow/structural/beams/engine_structural.beam")] ++ Path.wildcard(Path.join(root,".cache/whole_modules/fixtures/*.beam"))
end
true = length(paths) == 13
rows = for path <- paths do
  bytes = File.read!(path)
  beam_sha = Base.encode16(:crypto.hash(:sha256,bytes),case: :lower)
  module = CoreExport.module(path)
  directory = Path.join(destination,module.name)
  File.mkdir_p!(directory)
  {:ok, {_, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(String.to_charlist(path), [:abstract_code])
  File.write!(Path.join(directory,"oracle-source-forms.etf"), :erlang.term_to_binary(forms))
  warnings = :dialyzer.run(files: [String.to_charlist(path)], init_plt: String.to_charlist(base), check_plt: false, warnings: [:no_unknown], error_location: :column, indent_opt: false)
  core = %{version: 6, profile: CoreExport.profile(), atom_layout: CoreExport.atom_layout([module]), contract_closure_profile: contracts, modules: [module]}
  File.write!(Path.join(directory,"oracle-core.json"), :json.encode(core))
  File.write!(Path.join(directory,"oracle-warnings.txt"), Enum.map(warnings, &:dialyzer.format_warning(&1, indent_opt: false)))
  IO.inspect(%{module: module.name, warnings: length(warnings), beam: path})
  %{id: module.name, beam_path: path, beam_sha256: beam_sha, module: module.name, core: core, legal_warnings: tags, purged_modules: [], use_contracts: true, location: "column", solvers: [], warnings: CoreExport.export_terms(warnings), warning_count: length(warnings)}
end
File.write!(Path.join(destination,"oracle-row.jsonl"), Enum.map(rows, &[:json.encode(&1),"\n"]))
scope = %{actual_public_dialyzer_run: true, original_beams_not_recompiled: !("--bundled-fixtures" in options), native_frontend_verified: false, native_analysis_facts_as_frontend_input: false, cases: length(rows), warnings: Enum.sum(Enum.map(rows,& &1.warning_count)), prior_path: base, prior_sha256: base_sha, beams: Enum.map(rows,&Map.take(&1,[:id,:beam_path,:beam_sha256,:warning_count])), profile_scope: "captured installed OTP test context"}
File.write!(Path.join(destination,"capture-provenance.json"), :json.encode(scope))
