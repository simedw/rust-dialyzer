Code.require_file("core_export.exs", __DIR__)
{:ok, {_, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(:code.which(:dialyzer_iplt), [:abstract_code])
forms = Enum.map(forms, fn {:attribute, a, :module, _} -> {:attribute, a, :module, :fingerprint_native}; form -> form end)
{:ok, :fingerprint_native, binary, _} = :compile.forms(forms, [:binary, :export_all, :return_warnings])
{:module, :fingerprint_native} = :code.load_binary(:fingerprint_native, ~c"fingerprint_native", binary)
for line <- IO.stream(:stdio, :line) do
  row = :json.decode(line)
  proof_core = row["proof_core_json"]
  CoreExport.run(row["beams_dir"], proof_core)
  core = proof_core |> File.read!() |> :json.decode()
  modules = Enum.map(core["modules"], fn module ->
    {source, _} = CoreExport.rebuild_module(module)
    name = String.to_atom(module["name"])
    stable = Enum.flat_map(source, fn form -> case :fingerprint_native.make_stable(name, form) do {true, stable} -> [stable]; false -> [] end end)
    source_hash = :erlang.md5(:erlang.term_to_binary(stable))
    beam = Path.join(row["beams_dir"], module["name"] <> ".beam")
    native_hash = :fingerprint_native.compute_md5_from_file(String.to_charlist(beam))
    if source_hash != native_hash, do: raise("compiler source-form hash differs from native BEAM hash: #{beam}")
    %{name: module["name"], md5: Base.encode16(native_hash, case: :lower)}
  end)
  IO.puts(:json.encode(%{id: row["id"], modules: modules}))
end
