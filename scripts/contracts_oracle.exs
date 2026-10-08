Code.require_file("native_types.exs",__DIR__)
Code.require_file("core_export.exs",__DIR__)
defmodule ContractOracle do
 def decode(%{"kind"=>"atom","value"=>v}),do: String.to_atom(v)
 def decode(%{"kind"=>"integer","value"=>v}),do: String.to_integer(v)
 def decode(%{"kind"=>"nil"}),do: []
 def decode(%{"kind"=>"tuple","items"=>xs}),do: List.to_tuple(Enum.map(xs,&decode/1))
 def decode(%{"kind"=>"list","items"=>xs,"tail"=>tail}),do: Enum.reduce(Enum.reverse(xs),decode(tail),fn x,t->[decode(x)|t]end)
 def run(r,contracts) do
  c=Enum.at(contracts,r["contract_index"])["contract"]
  pairs=Enum.map(c["alternatives"],fn a->{decode(a["signature"]),Enum.map(a["constraints"],fn p->{:subtype,decode(p["left"]),decode(p["right"])}end)}end)
  actual=if r["arguments"],do: Enum.map(r["arguments"],&NativeTypes.decode/1),else: Enum.map(c["arguments"],&decode/1)
  contract={:contract,pairs,Enum.map(c["arguments"],&decode/1),[]}
  {actual,:dialyzer_contracts.get_contract_return(contract,actual)}
 end
end
[source,requests]=System.argv()
contracts=source|>File.read!()|>:json.decode()|>Map.fetch!("contracts")
results=requests|>File.stream!()|>Enum.map(fn line->ContractOracle.run(:json.decode(line),contracts)end)
IO.puts(:json.encode(CoreExport.export_terms(results)))
