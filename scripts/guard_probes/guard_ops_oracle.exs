Code.require_file("type_codec.exs", __DIR__)
Code.require_file("type_wire.exs", __DIR__)
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_dataflow),[:abstract_code])
forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:rust_guard_ops_native}; f->f end)
{:ok,:rust_guard_ops_native,bin,_}=:compile.forms(forms,[:binary,:export_all,:return_warnings])
{:module,:rust_guard_ops_native}=:code.load_binary(:rust_guard_ops_native,~c"guard_ops_oracle",bin)
defmodule GuardOpsOracle do
 def run(row) do
  try do
   eval=case row["eval"] do "positive"->:pos; "negative"->:neg; _->:dont_know end
   result=case row["op"] do
    "test_domain"->%{kind: "type",value: NativeTypeWire.encode(:rust_guard_ops_native.type_test_type(String.to_atom(row["name"]),row["arity"]))}
    "eval_inf"->%{kind: "type",value: NativeTypeWire.encode(:rust_guard_ops_native.guard_eval_inf(eval,TypeCodec.type(row["actual"])))}
    "bind_test"->case :rust_guard_ops_native.bind_type_test(eval,TypeCodec.type(row["expected"]),TypeCodec.type(row["actual"])) do
     :error->%{kind: "impossible"}
     {:ok,arg,result}->%{kind: "refined",argument: NativeTypeWire.encode(arg),result: NativeTypeWire.encode(result)}
    end
   end
   %{id: row["id"],ok: result}
  catch
   :error, reason when reason in [:function_clause,:badarg]->%{id: row["id"],error: %{kind: "error",class: Atom.to_string(reason)}}
   class,reason->raise "unclassified native exception #{inspect({class,reason})}"
  end
 end
end
for line<-IO.stream(:stdio,:line),do: IO.puts(:json.encode(GuardOpsOracle.run(:json.decode(line))))
