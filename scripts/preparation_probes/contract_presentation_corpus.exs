Code.require_file("../core_export.exs",__DIR__)
[captures,dependency,dir]=System.argv();File.mkdir_p!(dir)
own=File.stream!(captures)|>Enum.flat_map(fn line->r=:json.decode(line);for c<-r["contracts"],do: {%{mfa: c["mfa"],callback: c["callback"]},CoreExport.rebuild_terms(c["capture"]),nil}end)
library=dependency|>File.read!()|>:json.decode();external=for c<-library["contracts"]++library["callbacks"] do
 forms=CoreExport.rebuild_terms(c["contract"]["forms"]);cs=List.duplicate({:not_inspected,:not_inspected},length(c["contract"]["alternatives"]))
 {Map.take(c,["mfa"]),{:contract,cs,[],forms},c["contract"]}
end
rows=for {id,raw,dep}<-own++external,op<-[:contract_to_string,:is_overloaded] do
 result=try do v=apply(:dialyzer_contracts,op,[raw]);%{ok: if(is_list(v),do: List.to_string(v),else: v)}catch k,e->%{kind: Atom.to_string(k),reason: CoreExport.export_terms(e)}end
 request=if dep,do: %{id: id,op: Atom.to_string(op),dependency: dep},else: %{id: id,op: Atom.to_string(op),arena: CoreExport.export_terms(raw)}
 {request,result}
end
File.write!(Path.join(dir,"requests.jsonl"),Enum.map(rows,fn {r,_}->[:json.encode(r),"\n"]end));File.write!(Path.join(dir,"native.jsonl"),Enum.map(rows,fn {_,r}->[:json.encode(r),"\n"]end))
IO.inspect(%{own: length(own),library: length(external),operations: length(rows)})
