Code.require_file("core_export.exs", __DIR__)
defmodule ContextBoundaryReference do
 def drain(acc \\ []) do
  receive do {pid,:ext_types,event} when pid==self()->drain([event|acc]) after 0->Enum.reverse(acc) end
 end
 def run(c) do
  tables=:ets.new(:tables,[:set]);exports=:ets.new(:exports,[:set])
  snapshot=CoreExport.rebuild_terms(hd(c["snapshots"]))
  for e<-snapshot,do: :ets.insert(tables,e)
  for e<-CoreExport.rebuild_terms(c["exports"]),do: :ets.insert(exports,{e})
  step=hd(c["steps"]);s=step["site"]
  site={String.to_atom(s["kind"]),{String.to_atom(s["module"]),String.to_atom(s["name"]),s["arity"]},"cache.erl"}
  form=CoreExport.rebuild_terms(step["form"])
  drain()
  outcome=try do
   {v,cache}=case step["mode"] do
    "resolve" -> :erl_types.t_from_form(form,exports,site,tables,%{},:erl_types.cache__new())
    "check_fields" -> {:ok,:erl_types.t_check_record_fields(form,exports,site,tables,%{},:erl_types.cache__new())}
    _ -> :erl_types.t_from_form_check_remote(form,exports,site,tables);{:ok,:erl_types.cache__new()}
   end
   %{kind: "value",value: CoreExport.term(v),cache: CoreExport.term(cache)}
  catch kind,reason->%{kind: Atom.to_string(kind),reason: CoreExport.term(reason)} end
  events=drain();:ets.delete(tables);:ets.delete(exports)
  Map.merge(outcome,%{label: c["label"],events: CoreExport.term(events)})
 end
end
[cases]=System.argv()
Enum.each(File.stream!(cases),fn line->IO.puts(:json.encode(ContextBoundaryReference.run(:json.decode(line))))end)
