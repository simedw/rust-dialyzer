[root]=System.argv()
true=Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:dialyzer_typesig)))))=="08945A69DD63A44CE877D696E1C27BE2753DE29AE6FC1F8E183D37AB0ADD9882"
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/core_export.exs"))
for line<-IO.stream(:stdio,:line) do
 r=:json.decode(line);core=r["core_json"]|>File.read!()|>:json.decode()
 {modules,_}=Enum.map_reduce(Enum.with_index(core["modules"]),0,fn {module,index},next->
  {forms,tree}=CoreExport.rebuild_module(module);{labeled,next}=:cerl_trees.label(tree,next)
  exported=CoreExport.export_module(String.to_atom(module["name"]),forms,labeled)|>:json.encode()|>IO.iodata_to_binary()|>:json.decode()
  true=length(exported["nodes"])==length(module["nodes"])
  nodes=CoreExport.rebuild_nodes(exported)
  results=exported["nodes"]|>Enum.with_index()|>Enum.flat_map(fn {node,id}->
   if node["kind"]=="clause" do
    clause=Map.fetch!(nodes,id)
    result=try do
     %{type: NativeTypeWire.encode(:dialyzer_typesig.get_safe_underapprox(:cerl.clause_pats(clause),:cerl.clause_guard(clause)))}
    catch
     :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}}
     :error,:badarg->%{error: %{kind: "error",class: "badarg"}}
     :error,{:badmatch,b} when is_boolean(b)->%{error: %{kind: "error",class: "badmatch",payload: %{kind: "boolean",value: b}}}
    end
    [Map.merge(result,%{module: index,node: id})]
   else [] end
  end)
  {results,next}
 end)
 IO.puts(:json.encode(%{id: r["id"],clauses: List.flatten(modules)}))
end
