Code.require_file("../core_export.exs",__DIR__)
Code.require_file("../type_codec.exs",__DIR__)
Code.require_file("../type_wire.exs",__DIR__)
source=File.read!(Path.join(__DIR__,"../pattern_probes/binder_oracle.exs"))|>String.split("fields=BinderOracle.load()")|>hd()|>String.split("defmodule BinderOracle")|>List.last()|>String.split(" def run")|>hd()
Code.eval_string("defmodule BinderOracle"<>source<>"end")
fields=BinderOracle.load()
x=:cerl.c_var(:X);y=:cerl.c_var(:Y)
expression=:cerl.c_call(:cerl.c_atom(:erlang),:cerl.c_atom(:is_function),[x,y])
tree=:cerl.c_module(:cerl.c_atom(:fixture),[:cerl.c_fname(:fixture,2)],[],[{:cerl.c_fname(:fixture,2),:cerl.c_fun([x,y],expression)}]);{tree,_}=:cerl_trees.label(tree,0)
[{_,fun}]=:cerl.module_defs(tree);body=:cerl.fun_body(fun)
for eval<-[:pos,:neg,:dont_know],warnings<-[false,true] do
 state=:rust_binder_native.binder_state(:fixture,%{},warnings)
 map=:cerl_trees.fold(fn node,map->if :cerl.is_c_var(node) and :cerl.var_name(node) in [:X,:Y],do: :rust_binder_native.store_map(:cerl_trees.get_label(node),if(:cerl.var_name(node)==:X,do: :erl_types.t_any(),else: :erl_types.t_unit()),map),else: map end,:rust_binder_native.map__new(),tree)
 task=Task.async(fn->:rust_binder_native.bind_guard(body,map,%{},eval,state) end)
 outcome=Task.yield(task,80)
 stack=if outcome==nil,do: Process.info(task.pid,:current_stacktrace),else: nil
 Task.shutdown(task,:brutal_kill)
 :dialyzer_callgraph.delete(BinderOracle.get(state,fields.state,:callgraph));:dialyzer_codeserver.delete(BinderOracle.get(state,fields.state,:codeserver))
 if outcome != nil,do: raise("Unit number getter unexpectedly returned: #{inspect(outcome)}")
 {:current_stacktrace,frames}=stack
 if not Enum.any?(frames,fn {m,f,_,_}->m==:erl_types and f==:number_vals end),do: raise("unexpected blocked native stack #{inspect(stack)}")
 IO.puts(:json.encode(%{eval: eval,warnings: warnings,observation: "deadline_in_number_vals",source_cycle: "number_vals(Unit) -> t_inf(Unit,number)=Unit -> number_vals(Unit)",native_frames: Enum.map(frames,&inspect/1)}))
end
