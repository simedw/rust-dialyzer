[root,out]=System.argv()
:logger.set_primary_config(:level,:error)
Code.require_file(Path.join(root,"scripts/core_export.exs"))
defmodule CacheNative do
 def atoms(t) when is_atom(t),do: [t]
 def atoms(t) when is_tuple(t),do: t|>Tuple.to_list()|>Enum.flat_map(&atoms/1)
 def atoms(t) when is_map(t),do: Enum.flat_map(t,fn {k,v}->atoms(k)++atoms(v)end)
 def atoms([h|t]),do: atoms(h)++atoms(t)
 def atoms(_),do: []
 def warning(origin,n),do: {:warn_contract_types,{~c"cache.erl",{n,2},origin},{:invalid_contract,[n]}}
 def expected(fun) do
  try do %{value: CoreExport.export_terms(fun.())} catch k,r->%{error: %{class: to_string(k),reason: CoreExport.export_terms(r)}} end
 end
 def produce() do
  keys=[:a,:b,1,1.0,0.0,-0.0,<<5::3>>,<<1,2>>,{},[],[1],{:m,1},%{1=>2},999999999999999999999]
  basic=for {key,i}<-Enum.with_index(keys), old_mode<-0..3, collected_mode<-0..2, unknown_mode<-0..1 do
   origin=if rem(i,2)==0,do: {key,:f,0},else: key
   collected=case collected_mode do 0->:none;1->[];2->[warning(origin,1),warning(origin,2)] end
   unknown=if unknown_mode==0,do: [],else: [{key,warning({:unrelated,:f,0},3)}]
   old=case old_mode do 0->:none;1->%{};2->%{key=>[warning(origin,8)]};3->%{:retained=>[warning(:retained,9)]} end
   %{id: "basic/#{i}/#{old_mode}/#{collected_mode}/#{unknown_mode}",collected: collected,unknown: unknown,old: old,modules: [key]}
  end
  large=for size<-[1,2,31,32,33,64], mode<-0..2 do
   mods=for n<-1..size,do: String.to_atom("cache_module_#{n}")
   warnings=for {m,n}<-Enum.with_index(mods,1),do: warning({m,:f,0},n)
   %{id: "map/#{size}/#{mode}",collected: warnings,unknown: [],old: %{},modules: case mode do 0->[];1->Enum.reverse(mods);2->[hd(mods),hd(mods),:absent] end}
  end
  errors=for bad<-[{},:bad,{:tag,{},:msg},{:tag,{:file,1},:msg},{:tag,[:file,1,:m],:msg}] do
   %{id: "error/#{inspect(bad)}",collected: [bad],unknown: [],old: %{},modules: []}
  end
  malformed=for old<-[1,:bad,[],[1],%{:a=>1},%{:a=>[warning(:a,1)|:tail]},%{:a=>[warning(:a,1),warning(:a,2)|42]}], collected<-[:none,[],[warning(:a,3)],[:bad],:bad,[warning(:a,4)|:tail],[:bad|:tail]], unknown<-[[],[:bad],[{:a,:raw}], [{:a,:raw}|:tail],:bad], modules<-[[],[:a],[:absent]] do
   %{id: "late/#{inspect({old,collected,unknown,modules})}",collected: collected,unknown: unknown,old: old,modules: modules}
  end
  basic++large++errors++malformed
 end
end
{:ok,{:dialyzer_incremental,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_incremental),[:abstract_code])
{:function,_,:handle_return_and_print,3,[{:clause,_,_,_,body}]}=Enum.find(forms,fn form->match?({:function,_,:handle_return_and_print,3,_},form) end)
{:match,_,{:var,_,:WarningsToReport},selection}=Enum.find(body,fn form->match?({:match,_,{:var,_,:WarningsToReport},_},form) end)
selection_forms=[{:attribute,0,:module,:original_cache_select},{:attribute,0,:export,[{:select,2}]},{:function,0,:select,2,[{:clause,0,[{:var,0,:AllWarningsMap},{:var,0,:WarningModules}],[],[selection]}]}]
{:ok,:original_cache_select,beam}=:compile.forms(selection_forms,[:binary])
{:module,:original_cache_select}=:code.load_binary(:original_cache_select,~c"in_memory_original_expression",beam)
rows=for row<-CacheNative.produce() do
 result=CacheNative.expected(fn->:dialyzer_iplt.merge_warnings(row.collected,row.unknown,row.old)end)
 select=if Map.has_key?(result,:value) do
  raw=:dialyzer_iplt.merge_warnings(row.collected,row.unknown,row.old)
  if raw==:none,do: :null,else: CacheNative.expected(fn->:original_cache_select.select(raw,row.modules)end)
 else :null end
 purged=if Map.has_key?(result,:value),do: CacheNative.expected(fn->:maps.without(row.modules,:dialyzer_iplt.merge_warnings(row.collected,row.unknown,row.old))end),else: :null
 literals={row.collected,row.unknown,row.old,row.modules}
 %{id: row.id,collected: CoreExport.export_terms(row.collected),unknown: CoreExport.export_terms(row.unknown),old: CoreExport.export_terms(row.old),modules: CoreExport.export_terms(row.modules),layout: CompilerAtomPrimitives.export(CacheNative.atoms(literals),"cache-proof"),expected: result,selected: select,purged: purged}
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
