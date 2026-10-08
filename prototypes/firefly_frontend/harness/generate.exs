# Oracle generation only. No part of this script runs in the native frontend.
[root, out] = System.argv()
Code.require_file("core_export.exs", Path.join(root, "scripts"))
Code.require_file("dependency_export.exs", Path.join(root, "scripts"))
source = Path.join(out, "frontend_acceptance.erl")
beams = Path.join(out, "beams")
{:ok, :frontend_acceptance} = :compile.file(String.to_charlist(source), [:debug_info, {:outdir, String.to_charlist(beams)}])
beam = Path.join(beams, "frontend_acceptance.beam")
base = Path.join(root, ".cache/corpus/otp/base.plt")
plt = :dialyzer_cplt.from_file(String.to_charlist(base))
prior = try do ExportDependencies.build(plt, base) after :dialyzer_plt.delete(plt) end
File.write!(Path.join(out,"prior-library.json"), :json.encode(prior))
warnings = :dialyzer.run(files: [String.to_charlist(beam)], init_plt: String.to_charlist(base), check_plt: false, warnings: [:no_unknown], error_location: :column, indent_opt: false)
module = CoreExport.module(beam)
{:ok, {:frontend_acceptance, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(String.to_charlist(beam), [:abstract_code])
File.write!(Path.join(out, "oracle-source-forms.etf"), :erlang.term_to_binary(forms))
profile = fn name ->
  path = :code.which(name)
  {:ok, ^name, chunks} = :beam_lib.all_chunks(path)
  {:ok, {^name, [atoms: atoms]}} = :beam_lib.chunks(path, [:atoms])
  atoms = Map.new(atoms)
  {_, table} = List.keyfind(chunks, ~c"FunT", 0)
  <<count::32, entries::binary>> = table
  rows = for <<name::32, arity::32, label::32, index::32, free::32, uniq::32 <- entries>>, do: %{name: Atom.to_string(Map.fetch!(atoms,name)), arity: arity, label: label, index: index, env_count: free, uniq: uniq}
  true = length(rows) == count
  %{module: Atom.to_string(name), original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path)))), fun_table: Enum.with_index(rows, fn row,i -> Map.put(row,:new_index,i) end)}
end
closure = profile.(:dialyzer_typesig)
contracts = profile.(:dialyzer_contracts)
File.write!(Path.join(out,"closure-profile.json"), :json.encode(closure))
File.write!(Path.join(out,"contract-profile.json"), :json.encode(contracts))
core = %{version: 6, profile: CoreExport.profile(), atom_layout: CoreExport.atom_layout([module]), contract_closure_profile: contracts, modules: [module]}
File.write!(Path.join(out,"oracle-core.json"), :json.encode(core))
registry = Path.join(root,"corpus/warnings.json") |> File.read!() |> :json.decode()
tags = Enum.reject(registry["default"], &(&1 == "warn_unknown"))
row = %{id: "firefly-frontend-acceptance", core: core, legal_warnings: tags, purged_modules: [], use_contracts: true, location: "column", solvers: [], warnings: CoreExport.export_terms(warnings), warning_count: length(warnings)}
File.write!(Path.join(out,"oracle-row.jsonl"), [:json.encode(row),"\n"])
File.write!(Path.join(out,"oracle-warnings.txt"), Enum.map(warnings, &:dialyzer.format_warning(&1, indent_opt: false)))
IO.inspect(%{oracle_only: true, warnings: length(warnings), beam: beam, prior: base})
