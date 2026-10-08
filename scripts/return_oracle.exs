Code.require_file("core_export.exs", __DIR__)
{:ok, {:dialyzer_dataflow, [{:abstract_code, {:raw_abstract_v1, forms}}]}} = :beam_lib.chunks(:code.which(:dialyzer_dataflow), [:abstract_code])
{:ok, :dialyzer_dataflow, binary} = :compile.forms(forms, [:binary, :export_all])
{:module, :dialyzer_dataflow} = :code.load_binary(:dialyzer_dataflow, ~c"return_classifier_probe", binary)
for path <- IO.stream(:stdio, :line) do
  input = path |> String.trim() |> File.read!() |> :json.decode()
  output = Enum.map(input["modules"], fn mod ->
    {_, core} = CoreExport.rebuild_module(mod)
    nodes = CoreExport.rebuild_nodes(mod)
    functions = mod["nodes"] |> Enum.with_index() |> Enum.filter(fn {node,_}->node["kind"]=="fun" end) |> Enum.map(fn {_,id}->
      fun = Map.fetch!(nodes,id)
      %{node: id, kind: :dialyzer_dataflow.classify_returns(fun)}
    end)
    %{module: :cerl.module_name(core) |> :cerl.concrete(), functions: functions}
  end)
  IO.puts(:json.encode(output))
end
