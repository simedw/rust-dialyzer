Code.require_file("core_export.exs",__DIR__)
defmodule OpaqueOperandOracle do
 def result(value,module) do
  try do %{ok: :erl_types.t_is_opaque(value,module)}
  catch :error,{:case_clause,payload}->%{error: %{kind: "error",class: "case_clause"},reason: CoreExport.export_terms({:case_clause,payload})}; :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}}; c,r->raise "unclassified #{inspect({c,r})}" end
 end
end
plain=[:any,:none,:unit,:opaque,0,[],{:c,:atom,:any,:unknown},:cerl.c_atom(true),:cerl.c_atom(false),:cerl.c_int(1),:cerl.c_nil(),:cerl.c_tuple([:cerl.c_atom(:ok)])]
nominals=for m<-[:here,:there,9], opacity<-[:opaque,:transparent], qual<-[:unknown,:invalid],do: {:c,:nominal,{{m,:wrapped,0,opacity},:any},qual}
pool=plain++nominals
sets=for structure<-[:none,:any], members<-[[],pool,Enum.reverse(pool),:invalid,[0|:invalid],[List.last(nominals)|:invalid],[hd(nominals)|:invalid]],qual<-[:unknown,:invalid],do: {:c,:nominal_set,{members,structure},qual}
malformed=[{:c,:nominal,{:bad_name,:any},:unknown},{:c,:nominal_set,:bad_elements,:unknown},{:c,:nominal_set,{[hd(nominals)],:none},:unknown}]
for {value,i}<-Enum.with_index(pool++sets++malformed), module<-[:here,:there,:elsewhere] do
 IO.puts(:json.encode(Map.merge(%{id: "#{i}:#{module}",module: Atom.to_string(module),raw: CoreExport.export_terms(value)},OpaqueOperandOracle.result(value,module))))
end
