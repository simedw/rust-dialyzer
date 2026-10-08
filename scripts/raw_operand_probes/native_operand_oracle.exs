Code.require_file("core_export.exs",__DIR__)
defmodule NativeOperandOracle do
 def compare(c,r) do
  l=CoreExport.rebuild_terms(c["left"])
  right=if c["right"],do: CoreExport.rebuild_terms(c["right"]),else: nil
  result=try do
   value=if c["op"]=="impossible",do: :erl_types.t_is_impossible(l),else: :erl_types.t_inf(l,right)
   %{kind: "value",value: value}
  catch k,e->%{kind: Atom.to_string(k),reason: e}end
  cond do
   r["error"] && r["error"]["kind"]=="unsupported" -> %{status: "unsupported",native_kind: result.kind,detail: r["error"]["detail"]}
   result.kind=="value" && c["wire_identity"]=="right" ->
    %{status: if(:erts_debug.same(result.value,right) && c["right"]==r["ok"],do: "match",else: "mismatch"),proof: "native pointer identity + exact flat wire"}
   result.kind=="value" ->
    value=if c["op"]=="impossible",do: r["ok"],else: if(r["ok"],do: CoreExport.rebuild_terms(r["ok"]),else: :missing_rust_value)
    %{status: if(result.value===value,do: "match",else: "mismatch")}
   true ->
    reason=if r["error"] && r["error"]["reason"],do: CoreExport.rebuild_terms(r["error"]["reason"]),else: :missing_rust_reason
    %{status: if(r["error"] && r["error"]["kind"]==result.kind && reason===result.reason,do: "match",else: "mismatch"),native_kind: result.kind}
  end
 end
end
[commands,results]=System.argv();cs=Enum.to_list(File.stream!(commands));rs=Enum.to_list(File.stream!(results));if length(cs)!=length(rs),do: raise("row count mismatch")
{:ok,sup}=Task.Supervisor.start_link()
checks=Enum.zip(cs,rs)|>Enum.with_index()|>Enum.map(fn {{c,r},index}->
 if rem(index,1000)==0,do: IO.puts(:stderr,"native operand #{index}/#{length(cs)}")
 task=Task.Supervisor.async_nolink(sup,fn->NativeOperandOracle.compare(:json.decode(c),:json.decode(r))end)
 start=System.monotonic_time(:millisecond)
 case Task.yield(task,1000)||Task.shutdown(task,:brutal_kill) do
  {:ok,result}->result
  _->%{status: "oracle_timeout",elapsed_ms: System.monotonic_time(:millisecond)-start,index: index}
 end
end)
report=%{commands: length(checks),matches: Enum.count(checks,&(&1.status=="match")),mismatches: Enum.count(checks,&(&1.status=="mismatch")),unsupported: Enum.count(checks,&(&1.status=="unsupported")),oracle_timeouts: Enum.filter(checks,&(&1.status=="oracle_timeout")),failures: checks|>Enum.with_index()|>Enum.filter(fn {c,_}->c.status=="mismatch"end)|>Enum.map(fn {c,i}->Map.put(c,:index,i)end),unsupported_clauses: checks|>Enum.filter(&(&1.status=="unsupported"))|>Enum.frequencies_by(& &1.detail),full_native_operand_parity: Enum.all?(checks,&(&1.status=="match"))}
File.write!(results<>".comparison.json",:json.encode(report));IO.inspect(Map.drop(report,[:failures,:unsupported_clauses]));if report.mismatches>0,do: System.halt(1)
