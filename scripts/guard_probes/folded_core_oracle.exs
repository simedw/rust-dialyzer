Code.require_file("../core_export.exs",__DIR__)
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);original=row["core_json"]|>File.read!()|>:json.decode()
 {mods,results}=Enum.map_reduce(original["modules"],[],fn mod,acc->
  {forms,tree}=CoreExport.rebuild_module(mod)
  proof=CoreExport.export_module(String.to_atom(mod["name"]),forms,tree)|>:json.encode()|>IO.iodata_to_binary()|>:json.decode()
  nodes=CoreExport.rebuild_nodes(proof)
  rs=for {node,id}<-Enum.with_index(proof["nodes"]),node["kind"] in ["tuple","cons","map","var","literal"] do
   folded=:cerl.fold_literal(Map.fetch!(nodes,id))
   value=if :cerl.is_literal(folded),do: CoreExport.export_terms(:cerl.concrete(folded)),else: :null
   text=List.to_string(:cerl_prettypr.format(:cerl.set_ann(folded,[]),[{:hook,:dialyzer_utils.pp_hook()},:noann,{:paper,100000},{:ribbon,100000}]))
   %{node: id,literal: :cerl.is_literal(folded),value: value,text: text}
  end
  {proof,acc++[%{module: mod["name"],results: rs}]}
 end)
 IO.puts(:json.encode(%{id: row["id"],core: %{version: 5,profile: CoreExport.profile(),modules: mods},modules: results}))
end
