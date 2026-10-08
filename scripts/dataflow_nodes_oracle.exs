Code.require_file("core_export.exs", __DIR__)
source = File.read!(Path.join(__DIR__, "dataflow_map_oracle.exs")) |> String.split("fields=DataflowMapOracle.load()") |> hd()
Code.eval_string(source, [], file: Path.join(__DIR__, "dataflow_map_oracle.exs"))
fields = DataflowMapOracle.load()
for line <- IO.stream(:stdio, :line) do
  row = :json.decode(line)
  core = row["core_json"] |> File.read!() |> :json.decode()
  {modules, _} = Enum.map_reduce(core["modules"], 0, fn module, next ->
    {_, tree} = CoreExport.rebuild_module(module)
    {tree, next} = :cerl_trees.label(tree, next)
    map = :rust_dataflow_map_native.map__new() |> :rust_dataflow_map_native.join_maps_begin()
    map = :rust_dataflow_map_native.mark_as_fresh([tree], map)
    types = Map.new(DataflowMapOracle.get(map, fields, :map), fn {key, type} -> {Integer.to_string(key), NativeTypeWire.encode(type)} end)
    {%{name: module["name"], types: types, modified: DataflowMapOracle.get(map, fields, :modified)}, next}
  end)
  IO.puts(:json.encode(%{id: row["id"], modules: modules}))
end
