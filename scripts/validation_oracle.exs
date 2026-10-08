Code.require_file("forms_runtime.exs",__DIR__)
defmodule ValidationOracle do
 def native_summary(summary),do: :erl_types.t_fun(Enum.map(summary["arguments"],&CoreExport.decode(&1["native"])),CoreExport.decode(summary["result"]["native"]))
 def run(request) do
  reference=request["reference"]
  pairs=CoreExport.rebuild_terms(reference["types"])
  signatures=request["facts"]|>File.read!()|>:json.decode()|>Map.fetch!("signatures")
  signatures=Map.new(signatures,fn summary->{{summary["module"],summary["name"],summary["arity"]},native_summary(summary)}end)
  rows=Enum.zip(reference["contracts"],pairs)|>Enum.map(fn {contract,pairs}->
   [module,name,arity]=contract["mfa"]
   case Map.fetch(signatures,{module,name,arity}) do
    :error -> %{mfa: contract["mfa"],callback: contract["callback"],status: "missing_success"}
    {:ok,success} ->
     result=FormsOracle.result(fn -> :dialyzer_contracts.check_contract({:contract,pairs,[],[]},success,String.to_atom(module)) end)
     %{mfa: contract["mfa"],callback: contract["callback"],status: "compared",success: success,result: result}
   end
  end)
  arena=CoreExport.export_terms(Enum.map(rows,fn row->{row[:success]||[],get_in(row,[:result,:value])||[]}end))
  rows=Enum.map(rows,fn row->row|>Map.delete(:success)|>Map.update(:result,nil,fn r->if r,do: Map.drop(r,[:value,:missing]),else: nil end)end)
  %{rows: rows,types: arena}
 end
end
for line<-IO.stream(:stdio,:line),do: IO.puts(:json.encode(ValidationOracle.run(:json.decode(line))))
