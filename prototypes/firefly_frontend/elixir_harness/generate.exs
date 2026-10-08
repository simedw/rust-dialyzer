# Explicit VM oracle preparation. No VM calls are allowed in the native gate.
[root, out] = System.argv()
Code.require_file("core_export.exs", Path.join(root,"scripts"))
Code.require_file("dependency_export.exs", Path.join(root,"scripts"))
File.mkdir_p!(out)
base = Path.wildcard(Path.join(root,".cache/corpus/elixir/_elixir/base-*.plt")) |> List.first() || raise("missing Elixir base PLT")
plt = :dialyzer_cplt.from_file(String.to_charlist(base))
prior = try do ExportDependencies.build(plt, base) after :dialyzer_plt.delete(plt) end
File.write!(Path.join(out,"prior-library.json"), :json.encode(prior))
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
registry = Path.join(root,"corpus/warnings.json") |> File.read!() |> :json.decode()
selected = Path.join(root,".cache/corpus/elixir-selected.json") |> File.read!() |> :json.decode() |> Map.fetch!("cases")
cases = [%{"id"=>"tiny", "directory"=>Path.join(root,"examples/tiny/_build/dev/lib/tiny/ebin"), "warning_options"=>[]}] ++
  Enum.map(selected, fn c -> Map.put(c,"directory",Path.join(root,".cache/corpus/elixir/" <> String.replace(c["id"],"/","__") <> "/beams")) end)
rows = for c <- cases do
  id = c["id"]
  paths = Path.wildcard(Path.join(c["directory"],"*.beam"))
  if paths == [], do: raise("missing existing beams #{id}")
  modules = Enum.map(paths,&CoreExport.module/1) |> Enum.sort_by(& &1.name)
  # Use the suite's warning options and an immutable shared dependency PLT.
  options = Enum.map(c["warning_options"], &String.to_existing_atom/1)
  warnings = :dialyzer.run(files: Enum.map(paths,&String.to_charlist/1), init_plt: String.to_charlist(base), check_plt: false, warnings: [:no_unknown | options], error_location: :column, indent_opt: false)
  tags = Enum.reduce(c["warning_options"],registry["default"],fn option,acc -> change = Enum.find(registry["options"], &(&1["name"] == option)); if change["action"] == "disable", do: acc -- change["tags"], else: acc ++ change["tags"] end) |> Enum.uniq() |> Enum.reject(&(&1=="warn_unknown" and "unknown" not in c["warning_options"]))
  core = %{version: 6, profile: CoreExport.profile(), atom_layout: CoreExport.atom_layout(modules), contract_closure_profile: contracts, modules: modules}
  dir = Path.join(out,String.replace(id,"/","__"))
  File.mkdir_p!(dir)
  File.write!(Path.join(dir,"oracle-core.json"), :json.encode(core))
  File.write!(Path.join(dir,"oracle-warnings.txt"), Enum.map(warnings,&:dialyzer.format_warning(&1,indent_opt: false)))
  row = %{id: id, beam_paths: paths, beam_sha256: Map.new(paths,fn p->{p,Base.encode16(:crypto.hash(:sha256,File.read!(p)),case: :lower)} end), core: core, legal_warnings: tags, purged_modules: [], use_contracts: true, location: "column", solvers: [], warnings: CoreExport.export_terms(warnings), warning_count: length(warnings)}
  File.write!(Path.join(dir,"oracle-row.jsonl"), [:json.encode(row),"\n"])
  IO.inspect(%{id: id, modules: length(modules), warnings: length(warnings)})
  row
end
File.write!(Path.join(out,"oracle-rows.jsonl"), Enum.map(rows,&[:json.encode(&1),"\n"]))
File.write!(Path.join(out,"capture-provenance.json"), :json.encode(%{original_beams_not_recompiled: true, actual_public_dialyzer: true, native_frontend_verified: false, captured_runtime_profiles: true, prior_path: base, cases: Enum.map(rows,&Map.take(&1,[:id,:beam_paths,:beam_sha256,:warning_count]))}))
