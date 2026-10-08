Code.require_file("../core_export.exs",__DIR__)
defmodule PreparationCompare do
 def native(%{"kind"=>"atom","value"=>a}),do: String.to_atom(a)
 def native(%{"kind"=>"integer","value"=>n}),do: String.to_integer(n)
 def native(%{"kind"=>"nil"}),do: []
 def native(%{"kind"=>"tuple","items"=>xs}),do: xs|>Enum.map(&native/1)|>List.to_tuple()
 def native(%{"kind"=>"list","items"=>xs,"tail"=>t}),do: Enum.reduce(Enum.reverse(xs),native(t),fn x,t->[native(x)|t]end)
 def rust(%{"term"=>"atom","value"=>a}),do: String.to_atom(a)
 def rust(%{"term"=>"integer","value"=>n}),do: String.to_integer(n)
 def rust(%{"term"=>"nil"}),do: []
 def rust(%{"term"=>"tuple","values"=>xs}),do: xs|>Enum.map(&rust/1)|>List.to_tuple()
 def rust(%{"term"=>"list","values"=>xs}),do: Enum.map(xs,&rust/1)
 def records(xs,fun),do: Map.new(xs,fn r->{{r["module"],r["tag"],length(r["fields"])},{r["field_names"],Enum.map(r["fields"],fun),fun.(r["prototype"])}}end)
 def contracts(xs),do: Map.new(xs,fn c->{{c["mfa"],c["callback"]},CoreExport.rebuild_terms(c["capture"])}end)
 def semantic_contract({:contract,cs,args,forms}),do: {:contract,Enum.map(cs,fn {t,vs}->{t,:lists.sort(vs)}end),args,forms}
 def semantic_contracts(cs),do: Map.new(cs,fn {k,v}->{k,semantic_contract(v)}end)
 def compare(n,r) do
  cond do
   n["status"]!=r["status"]->%{pass: false,detail: "status",native: n["status"],rust: r["status"],error: r["error"]}
   n["status"]=="ok" ->
    nr=records(n["records"],&native/1);rr=records(r["records"],&rust/1)
    nc=contracts(n["contracts"]);rc=contracts(r["contracts"])
    nt=Map.new(n["tables"]||%{},fn {k,v}->{k,CoreExport.rebuild_terms(v)}end);rt=Map.new(r["tables"]||%{},fn {k,v}->{k,CoreExport.rebuild_terms(v)}end)
    ne=Enum.map(n["events"],&CoreExport.rebuild_terms/1);re=Enum.map(r["events"],&CoreExport.rebuild_terms/1)
    %{pass: nr===rr&&nc===rc&&ne===re&&nt===rt,records: nr===rr,tables: nt===rt,contracts: nc===rc,semantic_contracts: semantic_contracts(nc)===semantic_contracts(rc),events: ne===re,record_count: map_size(nr),contract_count: map_size(nc),native_only_contracts: Map.keys(nc)--Map.keys(rc),contract_diffs: for({k,v}<-nc,Map.get(rc,k)!==v,do: inspect(k)),native_events: inspect(ne),rust_events: inspect(re)}
   true->
    reason=if r["error"]["reason"],do: CoreExport.rebuild_terms(r["error"]["reason"]),else: :unrepresented
    ne=Enum.map(n["events"],&CoreExport.rebuild_terms/1);re=Enum.map(r["events"],&CoreExport.rebuild_terms/1)
    %{pass: n["kind"]==r["error"]["kind"]&&CoreExport.rebuild_terms(n["reason"])===reason&&ne===re,native: n,rust: r}
  end
 end
end
[native,rust,out]=System.argv();ns=Enum.to_list(File.stream!(native));rs=Enum.to_list(File.stream!(rust));if length(ns)!=length(rs),do: raise("native/Rust row count mismatch")
rows=Enum.zip(ns,rs)|>Enum.with_index()|>Enum.map(fn {{n,r},i}->Map.put(PreparationCompare.compare(:json.decode(n),:json.decode(r)),:index,i)end)
report=%{cases: length(rows),matches: Enum.count(rows,& &1.pass),failures: Enum.filter(rows,&(!&1.pass)),records: Enum.reduce(rows,0,&(&1[:record_count]||0)+&2),contracts: Enum.reduce(rows,0,&(&1[:contract_count]||0)+&2)}
File.write!(out,:json.encode(report));IO.inspect(Map.drop(report,[:failures]));if length(report.failures)>0,do: System.halt(1)
