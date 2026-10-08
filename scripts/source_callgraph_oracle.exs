Code.require_file("core_export.exs",__DIR__)
defmodule GraphWire do
 def vertex(v) when is_integer(v),do: ["label",Integer.to_string(v)]
 def vertex(:top),do: ["top"]
 def vertex({m,f,a}),do: ["named",Atom.to_string(m),Atom.to_string(f),Integer.to_string(a)]
end
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);core=row["core_json"]|>File.read!()|>:json.decode();cg=:dialyzer_callgraph.new()
 File.mkdir_p!(".cache/source_callgraph/proof_cores");proof_path=Path.expand(".cache/source_callgraph/proof_cores/"<>Base.encode16(:crypto.hash(:sha256,row["id"]),case: :lower)<>".json")
 proof_modules=Enum.map(core["modules"],fn mod->{forms,tree}=CoreExport.rebuild_module(mod);CoreExport.export_module(String.to_atom(mod["name"]),forms,tree)end)
 File.write!(proof_path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: proof_modules}))
 try do
  {modules,{_,proofs}}=Enum.map_reduce(core["modules"],{0,[]},fn mod,{next,proofs}->
   {forms,tree}=CoreExport.rebuild_module(mod);proof=CoreExport.export_module(String.to_atom(mod["name"]),forms,tree);{tree,next}=:cerl_trees.label(tree,next)
   {confirmed,edges}=:dialyzer_callgraph.scan_core_tree(tree,cg);:dialyzer_callgraph.add_edges(edges,confirmed,cg)
   labels=:cerl_trees.fold(fn n,acc->if :cerl.type(n)==:fun,do: [:cerl_trees.get_label(n)|acc],else: acc end,[],tree)|>Enum.sort()
   keys=labels|>Enum.flat_map(fn l->case :dialyzer_callgraph.lookup_name(l,cg) do {:ok,mfa}->[l,mfa];:error->[l] end end)
   m=%{confirmed: Enum.map(confirmed,&GraphWire.vertex/1),edges: Enum.map(edges,fn {a,b}->[GraphWire.vertex(a),GraphWire.vertex(b)]end),escaping: labels|>Enum.filter(&:dialyzer_callgraph.is_escaping(&1,cg))|>Enum.map(&Integer.to_string/1),self_recursive: keys|>Enum.filter(&:dialyzer_callgraph.is_self_rec(&1,cg))|>Enum.sort()|>Enum.map(&GraphWire.vertex/1),labels: labels}
   {m,{next,[proof|proofs]}}
  end)
  modules=Enum.map(modules,fn m->local=MapSet.new(m.labels);incoming=Map.new(m.labels,fn l->callers=case :dialyzer_callgraph.in_neighbours(l,cg) do :none->[];xs->xs end;callers=callers|>Enum.flat_map(fn k->case :dialyzer_callgraph.lookup_label(k,cg) do {:ok,l}->if MapSet.member?(local,l),do: [Integer.to_string(l)],else: [];:error->[] end end);{Integer.to_string(l),callers}end);m|>Map.delete(:labels)|>Map.put(:incoming,incoming)end)
  {cg,_bad_calls}=:dialyzer_callgraph.remove_external(cg)
  {sccs,_cg}=:dialyzer_callgraph.finalize(cg)
  components=Enum.map(sccs,fn {_id,vertices}->Enum.map(vertices,&GraphWire.vertex/1)|>Enum.sort_by(fn v->IO.iodata_to_binary(:json.encode(v))end)end)|>Enum.sort_by(fn v->IO.iodata_to_binary(:json.encode(v))end)
  File.mkdir_p!(".cache/source_callgraph/proof_cores");path=Path.expand(".cache/source_callgraph/proof_cores/"<>Base.encode16(:crypto.hash(:sha256,row["id"]),case: :lower)<>".json")
  File.write!(path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: Enum.reverse(proofs)}))
  IO.puts(:json.encode(%{id: row["id"],modules: modules,components: components,proof_core_json: path}))
 catch
  :error,:function_clause->IO.puts(:json.encode(%{id: row["id"],error: %{kind: "error",class: "function_clause"},proof_core_json: proof_path}))
 after :dialyzer_callgraph.delete(cg) end
end
