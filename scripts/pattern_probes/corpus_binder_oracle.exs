# Original OTP binder, with actual Core nodes and a deliberately broad entry map.
Code.require_file("../core_export.exs",__DIR__)
source=File.read!(Path.join(__DIR__,"binder_oracle.exs"))|>String.split("fields=BinderOracle.load()")|>hd()
Code.eval_string(source,[],file: Path.join(__DIR__,"binder_oracle.exs"))
fields=BinderOracle.load()
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);core=row["core_json"]|>File.read!()|>:json.decode()
 {modules,{_,proof_modules}}=Enum.map_reduce(core["modules"],{0,[]},fn mod,{next,proof_modules}->
  {forms,tree}=CoreExport.rebuild_module(mod)
  proof=CoreExport.export_module(String.to_atom(mod["name"]),forms,tree)
  {tree,next}=:cerl_trees.label(tree,next)
  labeled=CoreExport.export_module(String.to_atom(mod["name"]),forms,tree)|>:json.encode()|>IO.iodata_to_binary()|>:json.decode()
  if length(labeled["nodes"])!=length(mod["nodes"]),do: raise("node identity changed during labeling")
  nodes=CoreExport.rebuild_nodes(labeled)
  initial=:cerl_trees.fold(fn node,map->if :cerl.is_c_var(node),do: :rust_binder_native.enter_type(node,:erl_types.t_any(),map),else: map end,:rust_binder_native.map__new(),tree)
  state=:rust_binder_native.binder_state(String.to_atom(mod["name"]),%{},false)
  results=try do
   for {node,index}<-Enum.with_index(mod["nodes"]),node["kind"] in ["clause","fun"],roots=hd(node["groups"]),roots != [],reverse<-[false,true] do
    pats=Enum.map(roots,&Map.fetch!(nodes,&1));map=:rust_binder_native.mark_as_fresh(pats,initial)
    result=try do
     case :rust_binder_native.bind_pat_vars(pats,List.duplicate(:erl_types.t_any(),length(pats)),map,state,reverse) do
      {:error,kind,failed,actual}->%{status: "failure",kind: kind,actual: NativeTypeWire.encode(actual),patterns: List.to_string(:rust_binder_native.format_patterns(failed))}
      {map,types,state}->%{status: "ok",types: Enum.map(types,&NativeTypeWire.encode/1),changed: Map.new(BinderOracle.get(map,fields.map,:map),fn {key,type}->{Integer.to_string(key),NativeTypeWire.encode(type)} end)|>Map.reject(fn {_k,t}->t==%{"kind"=>"any"}end),substitutions: Map.new(BinderOracle.get(map,fields.map,:subst),fn {key,value}->{Integer.to_string(key),Integer.to_string(value)}end),warnings: CoreExport.export_terms(BinderOracle.get(state,fields.state,:warnings))}
     end
    catch kind,reason->%{status: "exception",class: kind,reason: CoreExport.export_terms(reason),stack: inspect(Enum.take(__STACKTRACE__,5))} end
    Map.merge(result,%{node: index,reverse: reverse,pattern_count: length(roots)})
   end
  after
   :dialyzer_callgraph.delete(BinderOracle.get(state,fields.state,:callgraph));:dialyzer_codeserver.delete(BinderOracle.get(state,fields.state,:codeserver))
  end
  {%{module: mod["name"],results: results},{next,[proof|proof_modules]}}
 end)
 File.mkdir_p!("data/corpus_proof_cores")
 proof_path=Path.expand("data/corpus_proof_cores/"<>Base.encode16(:crypto.hash(:sha256,row["id"]),case: :lower)<>".json")
 File.write!(proof_path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: Enum.reverse(proof_modules)}))
 IO.puts(:json.encode(%{id: row["id"],modules: modules,proof_core_json: proof_path}))
end
