Code.require_file("core_export.exs",__DIR__)
defmodule DeferredContextReference do
 def drain(acc \\ []) do
  receive do {pid,:ext_types,event} when pid==self()->drain([event|acc]) after 0->Enum.reverse(acc) end
 end
 def run(c) do
  tables=:ets.new(:tables,[:set]);exports=:ets.new(:exports,[:set])
  for e<-CoreExport.rebuild_terms(c["exports"]),do: :ets.insert(exports,{e})
  {_,results}=Enum.reduce(c["steps"],{:erl_types.cache__new(),[]},fn s,{cache,results}->
   :ets.delete_all_objects(tables)
   snapshot=CoreExport.rebuild_terms(Enum.at(c["snapshots"],s["snapshot"]));for e<-snapshot,do: :ets.insert(tables,e)
   cache=if s["fresh"],do: :erl_types.cache__new(),else: cache
   cache=if s["clear_module_cache"],do: put_elem(cache,2,%{}),else: cache
   site={:spec,{:m,:probe,0},"cache.erl"};form=CoreExport.rebuild_terms(s["form"]);drain()
   {next,r}=try do
    {v,next}=case s["mode"] do
     "check_fields" -> {:ok,:erl_types.t_check_record_fields(form,exports,site,tables,%{},cache)}
     "check_remote" -> :erl_types.t_from_form_check_remote(form,exports,site,tables);{:ok,cache}
     _ -> :erl_types.t_from_form(form,exports,site,tables,%{},cache)
    end
    {next,%{kind: "value",value: CoreExport.term(v),cache: CoreExport.term(next)}}
   catch kind,reason->{cache,%{kind: Atom.to_string(kind),reason: CoreExport.term(reason)}}end
   {next,[Map.put(r,:events,CoreExport.term(drain()))|results]}
  end)
  :ets.delete(tables);:ets.delete(exports)
  %{label: c["label"],results: Enum.reverse(results)}
 end
end
[input,output]=System.argv();File.open!(output,[:write],fn io->Enum.each(File.stream!(input),fn s->IO.binwrite(io,[:json.encode(DeferredContextReference.run(:json.decode(s))),"\n"])end)end)
