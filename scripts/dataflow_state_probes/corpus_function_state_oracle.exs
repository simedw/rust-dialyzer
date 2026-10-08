Code.require_file("core_export.exs",__DIR__)
source=File.read!(Path.join(__DIR__,"function_state_oracle.exs"))|>String.split("fields=FunctionStateOracle.load()")|>hd()
Code.eval_string(source,[],file: Path.join(__DIR__,"function_state_oracle.exs"))
{state_fields,_}=FunctionStateOracle.load()
for line<-IO.stream(:stdio,:line) do
 row=:json.decode(line);core=row["core_json"]|>File.read!()|>:json.decode()
 {modules,{_,proof_modules}}=Enum.map_reduce(core["modules"],{0,[]},fn mod,{next,proofs}->
  {forms,tree}=CoreExport.rebuild_module(mod);proof=CoreExport.export_module(String.to_atom(mod["name"]),forms,tree);{tree,next}=:cerl_trees.label(tree,next)
  cg=:dialyzer_callgraph.new();plt=:dialyzer_plt.new();codeserver=:dialyzer_codeserver.new()
  try do
   {names,edges}=:dialyzer_callgraph.scan_core_tree(tree,cg);:dialyzer_callgraph.add_edges(edges,names,cg)
   top=:cerl.ann_c_fun([{:label,:top}],[],tree)
   state=:rust_function_native.state__new(cg,codeserver,top,plt,String.to_atom(mod["name"]),%{})
   trees=FunctionStateOracle.get(state,state_fields,:tree_map);order=:dict.fetch_keys(trees)
   escaping=order|>Enum.filter(fn key->key != :top and :dialyzer_callgraph.is_escaping(key,cg) end)
   homes=Map.new(:dict.to_list(FunctionStateOracle.get(state,state_fields,:fun_homes)),fn {key,{m,f,a}}->{to_string(key),[Atom.to_string(m),Atom.to_string(f),a]}end)
   result=%{module: mod["name"],order: order,escaping: escaping,homes: homes,initial: FunctionStateOracle.snap(state,state_fields),warning: FunctionStateOracle.snap(:rust_function_native.state__set_warning_mode(state),state_fields)}
   {result,{next,[proof|proofs]}}
  after
   :dialyzer_callgraph.delete(cg);:dialyzer_plt.delete(plt);:dialyzer_codeserver.delete(codeserver)
  end
 end)
 File.mkdir_p!("data/function_proof_cores");path=Path.expand("data/function_proof_cores/"<>Base.encode16(:crypto.hash(:sha256,row["id"]),case: :lower)<>".json")
 File.write!(path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: Enum.reverse(proof_modules)}))
 IO.puts(:json.encode(%{id: row["id"],modules: modules,proof_core_json: path}))
end
