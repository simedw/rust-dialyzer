Code.require_file("core_export.exs",__DIR__)
for line<-IO.stream(:stdio,:line) do
  r=:json.decode(line)
  core=r["core_json"] |> File.read!() |> :json.decode()
  {modules,next}=Enum.map_reduce(core["modules"],r["initial"],fn module,next->
    {forms,tree}=CoreExport.rebuild_module(module)
    {labelled,next}=:cerl_trees.label(tree,next)
    exported=CoreExport.export_module(String.to_atom(module["name"]),forms,labelled) |> :json.encode() |> IO.iodata_to_binary() |> :json.decode()
    if length(exported["nodes"])!=length(module["nodes"]),do: raise("labelled Core shape changed")
    nodes=CoreExport.rebuild_nodes(exported)
    labels=Enum.map(0..(length(exported["nodes"])-1),fn id->
      case List.keyfind(:cerl.get_ann(Map.fetch!(nodes,id)),:label,0) do
        {:label,label}->label
        nil->:null
      end
    end)
    {%{name: module["name"],labels: labels},next}
  end)
  IO.puts(:json.encode(%{id: r["id"],next_label: next,modules: modules}))
end
