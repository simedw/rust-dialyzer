Code.require_file("../core_export.exs", __DIR__)
Code.require_file("../type_codec.exs", __DIR__)
Code.require_file("../type_wire.exs", __DIR__)
defmodule BinderOracle do
 def load do
  {:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_dataflow),[:abstract_code])
  fields=for {:attribute,_,:record,{name,fs}}<-forms,name in [:map,:state],into: %{} do
   {name,Enum.map(fs,fn f->f=if elem(f,0)==:typed_record_field,do: elem(f,1),else: f;elem(elem(f,2),2) end)}
  end
  {:ok,tokens,_}=:erl_scan.string(~c"binder_state(Module,Records,Warnings) -> #state{module=Module,records=Records,warning_mode=Warnings,callgraph=dialyzer_callgraph:new(),curr_fun={Module,fixture,0},codeserver=dialyzer_codeserver:new()}.")
  {:ok,wrapper}=:erl_parse.parse_form(tokens)
  forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:rust_binder_native};f->f end)++[wrapper]
  {:ok,:rust_binder_native,binary,_}=:compile.forms(forms,[:binary,:export_all,:return_warnings])
  {:module,:rust_binder_native}=:code.load_binary(:rust_binder_native,~c"binder_oracle",binary)
  {:ok,{_,[abstract_code: {:raw_abstract_v1,cgforms}]}}=:beam_lib.chunks(:code.which(:dialyzer_callgraph),[:abstract_code])
  {:attribute,_,:record,{:callgraph,cgfs}}=Enum.find(cgforms,&match?({:attribute,_,:record,{:callgraph,_}},&1))
  cgfields=Enum.map(cgfs,fn f->f=if elem(f,0)==:typed_record_field,do: elem(f,1),else: f;elem(elem(f,2),2) end)
  Map.put(fields,:callgraph,cgfields)
 end
 def get(record,fields,name),do: elem(record,Enum.find_index(fields,&(&1==name))+1)
 def map_wire(map,fields) do
  %{types: Map.new(get(map,fields,:map),fn {k,v}->{Integer.to_string(k),NativeTypeWire.encode(v)} end),substitutions: Map.new(get(map,fields,:subst),fn {k,v}->{Integer.to_string(k),if(is_integer(v),do: v,else: :cerl_trees.get_label(v))} end),modified: get(map,fields,:modified)}
 end
 def run(row,fields) do
  core=row["core"] || (row["core_json"] |> File.read!() |> :json.decode())
  {forms,root}=CoreExport.rebuild_module(core["modules"]|>hd())
  if path=row["proof_core_json"] do
   File.write!(path,:json.encode(%{version: 5,profile: CoreExport.profile(),modules: [CoreExport.export_module(:fixture,forms,root)]}))
  end
  {root,_}=:cerl_trees.label(root,0)
  [{_,fun}]=:cerl.module_defs(root)
  pats=:cerl.values_es(:cerl.fun_body(fun))
  map=:rust_binder_native.mark_as_fresh(pats,:rust_binder_native.map__new())
  map=Enum.reduce(row["initial"]||[],map,fn [key,type],map->:rust_binder_native.store_map(key,TypeCodec.type(type),map) end)
  records=Map.new(row["records"]||[],fn record->
   fields=Enum.with_index(record["fields"],fn t,i->{String.to_atom("field#{i}"),{:type,0,:any,[]},TypeCodec.type(t)} end)
   {{:record,String.to_atom(record["tag"])},{{~c"fixture",1},[{length(fields),fields}]}}
  end)
  state=:rust_binder_native.binder_state(:fixture,records,row["warnings"]||false)
  cg=get(state,fields.state,:callgraph);letrec_table=get(cg,fields.callgraph,:letrec_map)
  fun_tab=Enum.with_index(row["letrec"]||[])|>Enum.reduce(:dict.new(),fn {[key,type],index},tab->
   funlabel=70_000_000+index;:ets.insert(letrec_table,{key,funlabel});function=TypeCodec.type(type);args=:erl_types.t_fun_args(function);ret=:erl_types.t_fun_range(function);:dict.store(funlabel,{args,ret},tab)
  end)
  state=put_elem(state,Enum.find_index(fields.state,&(&1==:fun_tab))+1,fun_tab)
  try do
   result=:rust_binder_native.bind_pat_vars(pats,Enum.map(row["types"],&TypeCodec.type/1),map,state,row["reverse"]||false)
   case result do
    {:error,kind,failed,type}->%{id: row["id"],status: "failure",kind: kind,actual: NativeTypeWire.encode(type),patterns: List.to_string(:rust_binder_native.format_patterns(failed))}
    {map,types,state}->%{id: row["id"],status: "ok",map: map_wire(map,fields.map),types: Enum.map(types,&NativeTypeWire.encode/1),warnings: CoreExport.export_terms(get(state,fields.state,:warnings)),input_patterns: List.to_string(:rust_binder_native.format_patterns(pats)),origins: Enum.map(get(state,fields.state,:warnings),fn {_,{file,location,{m,f,a}},_}->%{file: List.to_string(file),location: if(is_tuple(location),do: Tuple.to_list(location),else: location),function: [Atom.to_string(m),Atom.to_string(f),a]} end),messages: Enum.map(get(state,fields.state,:warnings),fn {_,_,{_,args}}->Enum.map(args,&List.to_string/1) end)}
   end
  catch kind,reason->%{id: row["id"],status: "exception",class: kind,reason: CoreExport.export_terms(reason)}
  after
   :dialyzer_callgraph.delete(get(state,fields.state,:callgraph));:dialyzer_codeserver.delete(get(state,fields.state,:codeserver))
  end
 end
end
fields=BinderOracle.load()
for line<-IO.stream(:stdio,:line),do: IO.puts(:json.encode(BinderOracle.run(:json.decode(line),fields)))
