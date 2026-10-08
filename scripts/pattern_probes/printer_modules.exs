Code.require_file("../core_export.exs",__DIR__)
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);mod=hd(row["core"]["modules"]);{_,tree}=CoreExport.rebuild_module(mod)
 row=Map.merge(row,%{"id"=>row["id"]<>"-module","node"=>mod["core"],"native"=>List.to_string(:cerl_prettypr.format(:cerl.set_ann(tree,[]),[{:hook,:dialyzer_utils.pp_hook()},:noann,{:paper,100000},{:ribbon,100000}]))})
 IO.puts(:json.encode(row))
end
