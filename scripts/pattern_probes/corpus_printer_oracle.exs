Code.require_file("../core_export.exs",__DIR__)
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);core=row["core_json"]|>File.read!()|>:json.decode()
 modules=Enum.map(core["modules"],fn mod->
  {_,tree}=CoreExport.rebuild_module(mod)
  text=try do %{"Ok"=>List.to_string(:cerl_prettypr.format(:cerl.set_ann(tree,[]),[{:hook,:dialyzer_utils.pp_hook()},:noann,{:paper,100000},{:ribbon,100000}]))} catch kind,reason->%{"Err"=>inspect({kind,reason})}end
  %{module: mod["name"],text: text}
 end)
 IO.puts(:json.encode(%{id: row["id"],modules: modules}))
end
