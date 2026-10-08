Code.require_file("core_export.exs",__DIR__)
Code.require_file("cache_type_codec.exs",__DIR__)
Code.require_file("native_types.exs",__DIR__)
defmodule ContextStatusOracle do
 def decode(%{"term"=>"atom","value"=>v}),do: String.to_atom(v)
 def decode(%{"term"=>"integer","value"=>v}),do: String.to_integer(v)
 def decode(%{"term"=>"tuple","values"=>v}),do: v|>Enum.map(&decode/1)|>List.to_tuple()
 def decode(%{"term"=>"list","values"=>v}),do: Enum.map(v,&decode/1)
 def decode(%{"term"=>"nil"}),do: []
 def run(c,r) do
  snapshots=Enum.map(c["snapshots"],&CoreExport.rebuild_terms/1)
  tables=:ets.new(:cache_dict,[:set]);exports=:ets.new(:cache_exports,[:set])
  for e<-CoreExport.rebuild_terms(c["exports"]),do: :ets.insert(exports,{e})
  if length(c["steps"])!=length(r["ok"]),do: raise("step/output row count mismatch")
  {_,checks}=Enum.zip(c["steps"],r["ok"])|>Enum.reduce({:erl_types.cache__new(),[]},fn {step,got},{cache,checks}->
   :ets.delete_all_objects(tables);snapshot=Enum.at(snapshots,step["snapshot"])
   for entry<-snapshot,do: :ets.insert(tables,entry)
   form=CoreExport.rebuild_terms(step["form"]);module=String.to_atom(step["module"]||"m");site=case step["site"] do
    nil -> {:spec,{module,:probe,0},"cache.erl"}
    s -> {String.to_atom(s["kind"]),{String.to_atom(s["module"]||"m"),String.to_atom(s["name"]||"probe"),s["arity"]||0},"cache.erl"}
   end
   variables=Map.new(step["variables"]||[],fn v->name=case v["name"]["kind"] do "atom"->String.to_atom(v["name"]["value"]);"integer"->String.to_integer(v["name"]["value"])end;{name,ScalarOracle.type(v["type"])}end)
   cache=if step["fresh"],do: :erl_types.cache__new(),else: cache
   try do
    {value,next}=case step["mode"] do
     "resolve"->:erl_types.t_from_form(form,exports,site,tables,variables,cache)
     "without_remote"->{:erl_types.t_from_form_without_remote(form,site,Map.get(snapshot,module,%{})),cache}
     "check_remote"->:erl_types.t_from_form_check_remote(form,exports,site,tables);{:erl_types.t_atom(:ok),cache}
     "check_fields"->next=:erl_types.t_check_record_fields(form,exports,site,tables,variables,cache);{:erl_types.t_atom(:ok),next}
    end
    expected_cache=CoreExport.rebuild_terms(got["cache"])
    check=is_nil(got["error"]) and :erl_types.t_is_equal(value,decode(got["ok"])) and next===expected_cache
    {next,[%{parity: check,native: NativeTypes.term(value),rust: got["ok"],cache_parity: next===expected_cache}|checks]}
   catch k,e->
    expected_cache=CoreExport.rebuild_terms(got["cache"])
    {cache,[%{parity: not is_nil(got["error"]) and cache===expected_cache,native_rejection_only: true,native_error: inspect({k,e}),rust: got}|checks]}

   end
  end)
  :ets.delete(tables);:ets.delete(exports)
  Enum.reverse(checks)
 end
end
[p,r]=System.argv()
commands=Enum.to_list(File.stream!(p));outputs=Enum.to_list(File.stream!(r))
if length(commands)!=length(outputs),do: raise("input/output row count mismatch")
rows=Enum.zip(commands,outputs)|>Enum.map(fn {c,r}->ContextStatusOracle.run(:json.decode(c),:json.decode(r))end)
checks=List.flatten(rows);failed=Enum.count(checks,&(!&1.parity));report=%{cases: length(rows),outcomes: length(checks),matched: length(checks)-failed,failed: failed,failures: Enum.with_index(rows)|>Enum.flat_map(fn {r,i}->r|>Enum.with_index()|>Enum.filter(fn {c,_}->!c.parity end)|>Enum.map(fn {c,j}->Map.merge(c,%{case: i,step: j})end)end)}
File.write!(r<>".comparison.json",:json.encode(report));IO.inspect(Map.drop(report,[:failures]));if failed>0,do: raise("cache parity failures")
