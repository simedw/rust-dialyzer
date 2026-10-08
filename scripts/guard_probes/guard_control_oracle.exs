Code.require_file("../core_export.exs",__DIR__)
Code.require_file("../type_codec.exs",__DIR__)
Code.require_file("../type_wire.exs",__DIR__)
source=File.read!(Path.join(__DIR__,"../pattern_probes/binder_oracle.exs"))|>String.split("fields=BinderOracle.load()")|>hd()|>String.split("defmodule BinderOracle")|>List.last()|>String.split(" def run")|>hd()
Code.eval_string("defmodule BinderOracle"<>source<>"end")
fields=BinderOracle.load()
x=:cerl.c_var(:X);y=:cerl.c_var(:Y);a=:cerl.c_var(:A);b=:cerl.c_var(:B);e=:cerl.c_var(:E)
pair=fn op,key,val->:cerl.ann_c_map_pair([], :cerl.c_atom(op), key,val)end
expressions=[{"literal_true",:cerl.c_atom(true)},{"literal_false",:cerl.c_atom(false)},{"literal_atom",:cerl.c_atom(:ok)},{"literal_integer",:cerl.c_int(7)},{"literal_nil",:cerl.c_nil()},{"literal_map",:cerl.abstract(%{a: 7})},{"variable",x},{"values",:cerl.c_values([x,y])},{"values_twice",:cerl.c_values([x,x])},{"values_empty",:cerl.c_values([])},{"tuple",:cerl.c_tuple_skel([x,y])},{"tuple_empty",:cerl.c_tuple_skel([])},{"cons",:cerl.c_cons_skel(x,y)},{"proper_cons",:cerl.c_cons_skel(x,:cerl.c_nil())},{"binary",:cerl.c_binary([])},{"let",:cerl.c_let([a],x,a)},{"let_literal",:cerl.c_let([a],:cerl.c_atom(false),a)},{"let_constructor",:cerl.c_let([a],:cerl.c_tuple_skel([x,y]),a)},{"let_nested",:cerl.c_let([a],:cerl.c_let([b],x,b),a)},{"let_unused",:cerl.c_let([a],:cerl.c_call(:cerl.c_atom(:erlang),:cerl.c_atom(:unknown),[]),:cerl.c_atom(true))},{"try_false",:cerl.c_try(x,[a],a,[e],:cerl.c_atom(false))},{"try_true",:cerl.c_try(x,[a],a,[e],:cerl.c_atom(true))},{"try_var",:cerl.c_try(x,[a],a,[e],y)},{"try_fresh",:cerl.c_try(x,[a],a,[e],e)},{"try_binary",:cerl.c_try(x,[a],a,[e],:cerl.c_binary([]))},{"map_assoc",:cerl.ann_c_map([],x,[pair.(:assoc,:cerl.c_atom(:key),y)])},{"map_exact",:cerl.ann_c_map([],x,[pair.(:exact,:cerl.c_atom(:key),y)])},{"map_bad_op",:cerl.ann_c_map([],x,[pair.(:bad,:cerl.c_atom(:key),y)])},{"map_pairs",:cerl.ann_c_map([],:cerl.abstract(%{}),[pair.(:assoc,x,y),pair.(:exact,:cerl.c_atom(:key),x)])}]
pool=Enum.map(["any","none","unit","integer","atom","binary","nil","map_any"],&%{"kind"=>&1})++[%{"kind"=>"atoms","values"=>["true"]},%{"kind"=>"atoms","values"=>["false"]},%{"kind"=>"atoms","values"=>["true","false"]},%{"kind"=>"atoms","values"=>["ok"]}]
pool=pool++[%{"kind"=>"var","id"=>%{"atom"=>"A"}},%{"kind"=>"opaque"}]++(for inner<-[%{"kind"=>"integer"},%{"kind"=>"atoms","values"=>["true"]},%{"kind"=>"map_any"}],opacity<-["transparent","opaque"],do: %{"kind"=>"nominal","name"=>%{"module"=>"other","name"=>"wrapped","arity"=>0,"opacity"=>opacity},"inner"=>inner})
File.mkdir_p!(".cache/guard_control/control_proof_cores")
for {name,expression}<-expressions do
 tree=:cerl.c_module(:cerl.c_atom(:fixture),[:cerl.c_fname(:fixture,2)],[],[{:cerl.c_fname(:fixture,2),:cerl.c_fun([x,y],expression)}]);{tree,_}=:cerl_trees.label(tree,0)
 proof=CoreExport.export_module(:fixture,[],tree);path=Path.expand(".cache/guard_control/control_proof_cores/"<>name<>".json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [proof]}))
 [{_,fun}]=:cerl.module_defs(tree);body=:cerl.fun_body(fun)
 for {tx,i}<-Enum.with_index(pool),{ty,j}<-Enum.with_index(pool),eval<-[:pos,:neg,:dont_know] do
  map=:cerl_trees.fold(fn node,map->if :cerl.is_c_var(node) and :cerl.var_name(node) in [:X,:Y],do: :rust_binder_native.store_map(:cerl_trees.get_label(node),TypeCodec.type(if(:cerl.var_name(node)==:X,do: tx,else: ty)),map),else: map end,:rust_binder_native.map__new(),tree)
  state=:rust_binder_native.binder_state(:fixture,%{},false)
  result=try do
   {map,type,_}=:rust_binder_native.bind_guard(body,map,%{},eval,state)
   %{ok: %{map: BinderOracle.map_wire(map,fields.map),value: NativeTypeWire.encode(type)}}
  catch :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}};:throw,{:fail,:none}->%{failure: %{fatal: false}};:throw,{:fatal_fail,:none}->%{failure: %{fatal: true}};c,r->raise "unclassified #{inspect({name,c,r})}"
  after :dialyzer_callgraph.delete(BinderOracle.get(state,fields.state,:callgraph));:dialyzer_codeserver.delete(BinderOracle.get(state,fields.state,:codeserver))end
  row=%{id: "#{name}:#{i}:#{j}:#{eval}",core_json: path,x: tx,y: ty,eval: Atom.to_string(eval)}
  IO.puts(:json.encode(Map.merge(row,result)))
 end
end
