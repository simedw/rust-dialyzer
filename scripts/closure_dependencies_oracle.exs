Code.require_file("core_export.exs", __DIR__)
[core_path,rust_path,out]=System.argv()
program=core_path|>File.read!()|>:json.decode()
rust=rust_path|>File.read!()|>:json.decode()
labelled=Enum.map(Enum.with_index(program["modules"]),fn {m,index}->
  groups=m["nodes"] |> Enum.at(m["core"]) |> Map.fetch!("groups")
  skip=MapSet.new(Enum.at(groups,1)++Enum.take_every(Enum.at(groups,3),2))
  label_atom=length(m["terms"])
  terms=m["terms"]++[%{kind: "atom",value: "rust_node_id"}]
  {nodes,{extra,_}}=Enum.map_reduce(Enum.with_index(m["nodes"]),{[],length(terms)},fn {node,id},{extra,int}->
    pair=int+1;anno=int+2
    intterm=%{kind: "integer",value: Integer.to_string(id)}
    pairterm=%{kind: "tuple",items: [label_atom,int]}
    annoterm=%{kind: "list",items: [pair],tail: node["annotations"]}
    {if(MapSet.member?(skip,id),do: node,else: Map.put(node,"annotations",anno)),{[annoterm,pairterm,intterm|extra],int+3}}
  end)
  terms=terms++Enum.reverse(extra)
  m=Map.merge(m,%{"nodes"=>nodes,"terms"=>terms})
  {_forms,core}=CoreExport.rebuild_module(m |> :json.encode() |> IO.iodata_to_binary() |> :json.decode())
  {tree,_}=:cerl_trees.label(core)
  funcs=Map.new(Enum.filter(rust["functions"],&(&1["module"]==index)),&{&1["node"],&1["id"]})
  labels=:cerl_trees.fold(fn node,acc->
    label=Keyword.get(:cerl.get_ann(node),:label)
    id=Keyword.get(:cerl.get_ann(node),:rust_node_id)
    if label==nil, do: acc, else: (if id==nil, do: acc, else: Map.put(acc,label,id))
  end,%{},tree)
  function_labels=:cerl_trees.fold(fn node,acc->
    if :cerl.type(node)==:fun do
      id=Keyword.fetch!(:cerl.get_ann(node),:rust_node_id)
      Map.put(acc,:cerl_trees.get_label(node),Map.fetch!(funcs,id))
    else acc end
  end,%{},tree)
  vars=Map.new(Enum.filter(rust["variables"],&(&1["module"]==index)),&{&1["node"],&1["binding"]})
  {deps,esc,calls,letrecs}=:dialyzer_dep.analyze(tree)
  targets=fn values->%{external: :external in values,local: Enum.filter(values,&is_integer/1)|>Enum.map(&Map.fetch!(function_labels,&1))|>Enum.sort()}end
  deps=:dict.to_list(deps)|>Enum.map(fn {from,to}->[if(is_integer(from),do: Map.fetch!(function_labels,from),else: Atom.to_string(from)),targets.(to)]end)
  calls=:dict.to_list(calls)|>Enum.map(fn {from,to}->%{module: index,node: Map.fetch!(labels,from),targets: targets.(to)}end)
  letrecs=:dict.to_list(letrecs)|>Enum.map(fn {var,fun}->%{binding: vars[Map.fetch!(labels,var)],target: Map.fetch!(function_labels,fun)}end)
  %{module: index,dependencies: deps,escaping: targets.(esc),calls: calls,letrecs: letrecs}
end)
File.write!(out,:json.encode(labelled))
