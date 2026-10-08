[root,requests,out]=System.argv()
Code.require_file(Path.join(root,"scripts/core_export.exs"))
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
defmodule DiagnosticInput do
 def form(%{"kind"=>"function","domain"=>args,"range"=>range}),do: {:type,0,:fun,[{:type,0,:product,Enum.map(args,&form/1)},form(range)]}
 def form(%{"kind"=>"record_form","tag"=>tag}),do: {:type,0,:record,[{:atom,0,String.to_atom(tag)}]}
 def form(%{"kind"=>"local_form","name"=>name}),do: {:user_type,0,String.to_atom(name),[]}
 def form(%{"kind"=>"remote_form","module"=>m,"name"=>n}),do: {:remote_type,0,[{:atom,0,String.to_atom(m)},{:atom,0,String.to_atom(n)},[]]}
 def form(%{"kind"=>"atoms","values"=>names}),do: union(Enum.map(names,&{:atom,0,String.to_atom(&1)}))
 def form(%{"kind"=>"integers","values"=>ns}),do: union(Enum.map(ns,&{:integer,0,TypeCodec.integer(&1)}))
 def form(%{"kind"=>"tuple","elements"=>xs}),do: {:type,0,:tuple,Enum.map(xs,&form/1)}
 def form(%{"kind"=>"union","values"=>xs}),do: union(Enum.map(xs,&form/1))
 def form(%{"kind"=>"proper_list","elements"=>x,"nonempty"=>n}),do: {:type,0,if(n,do: :nonempty_list,else: :list),[form(x)]}
 def form(%{"kind"=>"var","id"=>%{"atom"=>v}}),do: {:var,0,String.to_atom(v)}
 def form(%{"kind"=>k}),do: {:type,0,case k do "unit"->:no_return;"any"->:any;"tuple_any"->:tuple;"map_any"->:map;other->String.to_atom(other)end,[]}
 def union([x]),do: x
 def union(xs),do: {:type,0,:union,xs}
 def contract(c) do
  pairs=Enum.map(c["alternatives"],fn alt->{TypeCodec.type(alt["signature"]),Enum.map(Map.get(alt,"constraints",[]),fn [l,r]->{:subtype,TypeCodec.type(l),TypeCodec.type(r)}end)}end)
  signatures=Enum.map(pairs,fn {t,cs}->map=Enum.reduce(cs,%{},fn {:subtype,l,r},acc->id=:erl_types.t_var_name(l);Map.update(acc,id,r,&:erl_types.t_inf(&1,r))end);:erl_types.t_subst(t,map)end)
  args=signatures|>:erl_types.t_sup()|>:erl_types.subst_all_vars_to_any()|>:erl_types.t_fun_args()
  forms=Enum.map(c["alternatives"],fn alt->
   constraints=Enum.map(Map.get(alt,"constraints",[]),fn [l,r]->{:type,0,:constraint,[{:atom,0,:is_subtype},[form(l),form(r)]]}end)
   {form(alt["form"]||alt["signature"]),constraints}
  end)
  {:contract,pairs,args,forms}
 end
 def table(mod) do
  file=String.to_charlist(mod["name"]<>".erl")
  records=Enum.map(Map.get(mod,"records",[]),fn r->fields=Enum.map(r["fields"],fn f->{String.to_atom(f["name"]),form(f["type"]),TypeCodec.type(f["type"])}end);{{:record,String.to_atom(r["tag"])},{{file,1},[{length(fields),fields}]}}end)
  aliases=Enum.map(Map.get(mod,"aliases",[]),fn a->{{:type,String.to_atom(a["name"]),0},{{String.to_atom(mod["name"]),{file,1},form(a["type"]),[]},:any}}end)
  Map.new(records++aliases)
 end
 def name(e),do: {String.to_atom(e["module"]),String.to_atom(e["function"]),e["arity"]}
 def fl(e),do: {String.to_charlist(e["file"]),location(e["location"])}
 def location([a,b]),do: {a,b}
 def location(a),do: a
 def entry(e),do: {name(e),{fl(e),contract(e),if(e["optional"],do: [:optional_callback],else: [])}}
 def wire(value),do: CoreExport.export_terms(value)
 def enrich(e),do: Map.merge(e,%{"capture"=>wire(contract(e)),"file_raw"=>wire(elem(fl(e),0)),"location_raw"=>wire(elem(fl(e),1))})
 def signature(e),do: {name(e),{TypeCodec.type(e["result"]),Enum.map(e["arguments"],&TypeCodec.type/1)}}
 def origin(e),do: {elem(fl(e),0),elem(fl(e),1),name(e)}
end
profile=Map.new([:dialyzer_contracts,:dialyzer_behaviours],fn m->{m,m|>:code.which()|>File.read!()|>then(&:crypto.hash(:sha256,&1))|>Base.encode16()}end)
rows=File.stream!(requests)|>Enum.map(fn line->
 r=:json.decode(line);cs=:dialyzer_codeserver.new();plt=:dialyzer_plt.new()
 :dialyzer_plt.insert_list(plt,Enum.map(r["signatures"],&DiagnosticInput.signature/1))
 if r["kind"]=="contracts" do
  modules=Enum.map(r["modules"],fn mod->
   map=Map.new(mod["entries"],&DiagnosticInput.entry/1)
   :dialyzer_codeserver.store_contracts(String.to_atom(mod["name"]),map,%{},cs)
   :ets.insert(:dialyzer_codeserver.get_records_table(cs),{String.to_atom(mod["name"]),DiagnosticInput.table(mod)})
   mod|>Map.put("table_raw",DiagnosticInput.wire(DiagnosticInput.table(mod)))|>Map.put("ordered_entries",Enum.map(:maps.to_list(map),fn {name,_}->mod["entries"]|>Enum.find(&(DiagnosticInput.name(&1)==name))|>DiagnosticInput.enrich()end))
  end)
  result=:dialyzer_contracts.get_invalid_contract_warnings(Enum.map(modules,&String.to_atom(&1["name"])),cs,plt)
  graph=:dialyzer_callgraph.new()
  signatures=Enum.with_index(r["signatures"])|>Enum.map(fn {sig,label}->:ets.insert(elem(graph,5),{label,DiagnosticInput.name(sig)});{label,:erl_types.t_fun(Enum.map(sig["arguments"],&TypeCodec.type/1),TypeCodec.type(sig["result"]))}end)
  contracts=modules|>Enum.flat_map(& &1["entries"])|>Enum.map(fn e->{DiagnosticInput.name(e),DiagnosticInput.contract(e)}end)|>:orddict.from_list()
  accepted=:dialyzer_contracts.check_contracts(contracts,graph,:orddict.from_list(signatures))
  :dialyzer_callgraph.delete(graph)
  :dialyzer_codeserver.delete(cs);:dialyzer_plt.delete(plt)
  Map.merge(r,%{"modules"=>modules,"expected"=>DiagnosticInput.wire(result),"accepted_expected"=>DiagnosticInput.wire(accepted),"profile"=>profile})
 else
  module=String.to_atom(r["module"])
  attrs=Enum.map(r["behaviours"],fn b->{:cerl.c_atom(if(b["american"],do: :behavior,else: :behaviour)),:cerl.ann_abstract([DiagnosticInput.location(b["location"])],[String.to_atom(b["name"])])}end)
  info=%{"module"=>r["module"],"function"=>"module_info","arity"=>0,"file"=>r["filename"],"location"=>1}
  functions=[info|r["functions"]]
  defs=Enum.with_index(functions)|>Enum.map(fn {e,i}->anns=[{:label,i},{:file,String.to_charlist(e["file"])},DiagnosticInput.location(e["location"])];{:cerl.c_fname(String.to_atom(e["function"]),e["arity"]),:cerl.ann_c_fun(anns,Enum.map(1..e["arity"]//1,fn n->:cerl.c_var(n)end),:cerl.c_atom(:ok))}end)
  :dialyzer_codeserver.insert(module,:cerl.c_module(:cerl.c_atom(module),[],attrs,defs),cs)
  :dialyzer_codeserver.insert_exports(Enum.map(r["exports"],&DiagnosticInput.name/1),cs)
  :dialyzer_codeserver.store_contracts(module,Map.new(r["declared"],&DiagnosticInput.entry/1),%{},cs)
  for {beh,entries}<-r["callbacks"],do: :dialyzer_codeserver.store_contracts(String.to_atom(beh),%{},Map.new(entries,&DiagnosticInput.entry/1),cs)
  :dialyzer_plt.insert_callbacks(plt,cs)
  callbacks=Map.new(r["callbacks"],fn {beh,entries}->{beh,case :dialyzer_plt.lookup_callbacks(plt,String.to_atom(beh))do :none->nil;{:value,list}->Enum.map(list,fn {name,_}->entries|>Enum.find(&(DiagnosticInput.name(&1)==name))|>DiagnosticInput.enrich()end)end}end)
  records=DiagnosticInput.table(Map.put(r,"name",r["module"]))
  result=:dialyzer_behaviours.check_callbacks(module,attrs,records,plt,cs)
  :dialyzer_codeserver.delete(cs);:dialyzer_plt.delete(plt)
  Map.merge(r,%{"ordered_callbacks"=>callbacks,"declared"=>Enum.map(r["declared"],&DiagnosticInput.enrich/1),"filename_raw"=>DiagnosticInput.wire(String.to_charlist(r["filename"])),"behaviours"=>Enum.map(r["behaviours"],&Map.put(&1,"location_raw",DiagnosticInput.wire(DiagnosticInput.location(&1["location"])))),"functions"=>Enum.map(r["functions"],&Map.put(&1,"origin_raw",DiagnosticInput.wire(DiagnosticInput.origin(&1)))),"expected"=>DiagnosticInput.wire(result),"profile"=>profile})
 end
end)
File.write!(out,Enum.map_join(rows,"\n",&:json.encode/1)<>"\n")
IO.inspect(%{cases: length(rows),profile: profile})
