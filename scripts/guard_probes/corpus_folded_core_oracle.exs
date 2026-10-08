Code.require_file("../core_export.exs",__DIR__)
File.mkdir_p!("data/fold_proof_cores")
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);original=row["core_json"]|>File.read!()|>:json.decode()
 {mods,results}=Enum.map_reduce(original["modules"],[],fn mod,acc->
  {forms,tree}=CoreExport.rebuild_module(mod)
  proof=CoreExport.export_module(String.to_atom(mod["name"]),forms,tree)|>:json.encode()|>IO.iodata_to_binary()|>:json.decode()
  nodes=CoreExport.rebuild_nodes(proof)
  values=for {node,id}<-Enum.with_index(proof["nodes"]),node["kind"] in ["tuple","cons"] do
   folded=:cerl.fold_literal(Map.fetch!(nodes,id))
   if :cerl.is_literal(folded),do: {id,true,:cerl.concrete(folded)},else: {id,false,:undefined}
  end
  {proof,acc++[%{module: mod["name"],outcomes: CoreExport.export_terms(values)}]}
 end)
 path=Path.expand("data/fold_proof_cores/"<>Base.encode16(:crypto.hash(:sha256,row["id"]),case: :lower)<>".json")
 File.write!(path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: mods}))
 IO.puts(:json.encode(%{id: row["id"],core_json: path,modules: results}))
end
