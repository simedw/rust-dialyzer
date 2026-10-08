Code.require_file("forms_runtime.exs",__DIR__)
Code.require_file("native_form_metadata.exs",__DIR__)
defmodule LocalContractsOracle do
 def run(request) do
  modules=request["core"]|>File.read!()|>:json.decode()|>Map.fetch!("modules")
  decoded=Enum.map(modules,&FormsOracle.decode/1)
  cores=Enum.zip_with(modules,decoded,&FormsOracle.core/2)
  {cs,tables,exports}=if request["finalized"] do
   NativeFormMetadata.prepare(modules,cores,decoded,request["environment"])
  else
   tables=:ets.new(:contract_types,[:set]);exports=:ets.new(:contract_exports,[:set])
   if request["environment"] do
    {base_tables,base_exports}=FormsOracle.environment(request["environment"])
    targets=MapSet.new(Enum.map(modules,&String.to_atom(&1["name"])))
    for {name,table}<-:maps.to_list(base_tables),not MapSet.member?(targets,name),do: :ets.insert(tables,{name,table})
    for {name,_,_}=entry<-base_exports,not MapSet.member?(targets,name),do: :ets.insert(exports,{entry})
   end
   Enum.zip(modules,cores)|>Enum.each(fn {module,core}->{:ok,table}=:dialyzer_utils.get_record_and_type_info(core);:ets.insert(tables,{String.to_atom(module["name"]),table})end)
   Enum.zip(modules,decoded)|>Enum.each(fn {module,array}->for {:attribute,_,:export_type,types}<-:array.get(module["source_forms"],array),{name,arity}<-types,do: :ets.insert(exports,{{String.to_atom(module["name"]),name,arity}})end)
   {nil,tables,exports}
  end
  rows=Enum.zip(modules,cores)|>Enum.flat_map(fn {module,core}->
   name=String.to_atom(module["name"]);table=case :ets.lookup(tables,name) do [{_,value}]->value;[]->%{} end
   {:ok,specs,callbacks}=:dialyzer_utils.get_spec_info(name,core,table)
   Enum.flat_map([{false,specs},{true,callbacks}],fn {callback,map}->
    Enum.map(map,fn {mfa,{_location,tmp,_flags}}->
     result=FormsOracle.result(fn ->
      {pairs,_}=Enum.map_reduce(elem(tmp,1),:erl_types.cache__new(),fn fun,cache->fun.(exports,tables,cache)end)
      pairs
     end)
     %{mfa: Tuple.to_list(mfa)|>Enum.map(fn v->if is_atom(v),do: Atom.to_string(v),else: v end),callback: callback,result: result}
    end)
   end)
  end)
  metadata=Map.new(modules,fn module->name=String.to_atom(module["name"]);table=case :ets.lookup(tables,name) do [{_,value}]->value;[]->%{} end;{name,table}end)
  if cs,do: :dialyzer_codeserver.delete(cs),else: (:ets.delete(tables);:ets.delete(exports))
  values=Enum.map(rows,fn r->get_in(r,[:result,:value])||[]end)
  missing=Enum.map(rows,fn r->r.result.missing end)
  rows=Enum.map(rows,fn r->Map.update!(r,:result,&Map.drop(&1,[:value,:missing]))end)
  %{profile: NativeFormMetadata.profile(),case: request["case"],contracts: rows,types: CoreExport.export_terms(values),metadata: CoreExport.export_terms(metadata),missing: CoreExport.export_terms(missing)}
 end
end
for line<-IO.stream(:stdio,:line),do: IO.puts(:json.encode(LocalContractsOracle.run(:json.decode(line))))
