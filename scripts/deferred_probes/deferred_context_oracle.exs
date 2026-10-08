Code.require_file("core_export.exs",__DIR__)
Code.require_file("cache_type_codec.exs",__DIR__)
defmodule BoundaryFullOracle do
 def decode(%{"term"=>"atom","value"=>v}),do: String.to_atom(v)
 def decode(%{"term"=>"integer","value"=>v}),do: String.to_integer(v)
 def decode(%{"term"=>"tuple","values"=>v}),do: v|>Enum.map(&decode/1)|>List.to_tuple()
 def decode(%{"term"=>"list","values"=>v}),do: Enum.map(v,&decode/1)
 def decode(%{"term"=>"nil"}),do: []
 def tree(%{"kind"=>"binary","value"=>v}),do: Base.decode64!(v)
 def tree(%{"kind"=>"float","value"=>v}) do
 <<n::float-64>>=Base.decode16!(v);n
 end
 def tree(%{"kind"=>"bitstring","value"=>v,"bits"=>bits}) do
 <<out::bitstring-size(bits),_::bitstring>>=Base.decode64!(v);out
 end
 def tree(%{"kind"=>"atom","value"=>v}),do: String.to_atom(v)
 def tree(%{"kind"=>"integer","value"=>v}),do: String.to_integer(v)
 def tree(%{"kind"=>"tuple","items"=>v}),do: List.to_tuple(Enum.map(v,&tree/1))
 def tree(%{"kind"=>"list","items"=>v,"tail"=>tail}),do: Enum.reduce(Enum.reverse(v),tree(tail),fn x,t->[tree(x)|t]end)
 def tree(%{"kind"=>"nil"}),do: []
 def tree(%{"kind"=>"map","entries"=>v}),do: Map.new(v,fn [k,val]->{tree(k),tree(val)}end)
 def check(n,r) do
  events=Enum.map(r["events"],&CoreExport.rebuild_terms/1)
  event_equal=events===tree(n["events"])
  outcome=case n["kind"] do
   "value"->
    expected=tree(n["value"])
    actual=if r["ok"],do: decode(r["ok"]),else: :missing_rust_value
    value=if actual==:missing_rust_value,do: false,else: if(expected==:ok,do: actual===:erl_types.t_atom(:ok),else: :erl_types.t_is_equal(expected,actual))
    is_nil(r["error"]) && value && CoreExport.rebuild_terms(r["cache"])===tree(n["cache"])
   kind->
    reason=if r["error"]["reason"],do: CoreExport.rebuild_terms(r["error"]["reason"]),else: String.to_atom(r["error"]["class"]||"unrepresented")
    r["error"]["kind"]==kind && reason===tree(n["reason"])
  end
  %{label: n["label"],parity: event_equal && outcome,events: event_equal,outcome: outcome}
 end
end
[native,rust]=System.argv()
commands=Enum.to_list(File.stream!(native));outputs=Enum.to_list(File.stream!(rust))
if length(commands)!=length(outputs),do: raise("input/output row count mismatch")
rows=Enum.zip(commands,outputs)|>Enum.flat_map(fn {n,r}->
 n=:json.decode(n);r=:json.decode(r);ns=n["results"];rs=r["ok"]
 if length(ns)!=length(rs),do: raise("step/output row count mismatch")
 Enum.zip(ns,rs)|>Enum.with_index()|>Enum.map(fn {{n,r},i}->BoundaryFullOracle.check(Map.put(n,"label",Integer.to_string(i)),r)end)
end)
report=%{cases: length(rows),failures: Enum.count(rows,&(!&1.parity)),results: rows}
IO.puts(:json.encode(report));File.write!(rust<>".full_comparison.json",:json.encode(report));if report.failures>0,do: System.halt(1)
