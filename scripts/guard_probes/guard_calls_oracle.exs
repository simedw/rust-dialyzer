Code.require_file("../core_export.exs",__DIR__)
Code.require_file("../type_codec.exs",__DIR__)
Code.require_file("../type_wire.exs",__DIR__)
source=File.read!(Path.join(__DIR__,"../pattern_probes/binder_oracle.exs"))|>String.split("fields=BinderOracle.load()")|>hd()|>String.split("defmodule BinderOracle")|>List.last()|>String.split(" def run")|>hd()
Code.eval_string("defmodule BinderOracle"<>source<>"end")
fields=BinderOracle.load()

x=:cerl.c_var(:X);y=:cerl.c_var(:Y)
call=fn f,args -> :cerl.c_call(:cerl.c_atom(:erlang),:cerl.c_atom(f),args) end
pool=Enum.map(["any","none","unit","integer","atom","binary","nil","map_any"],&%{"kind"=>&1})++[%{"kind"=>"atoms","values"=>["true"]},%{"kind"=>"atoms","values"=>["false"]},%{"kind"=>"atoms","values"=>["true","false"]},%{"kind"=>"atoms","values"=>["ok"]},%{"kind"=>"var","id"=>%{"atom"=>"A"}},%{"kind"=>"opaque"}]++(for inner<-[%{"kind"=>"integer"},%{"kind"=>"atoms","values"=>["true"]},%{"kind"=>"map_any"}],opacity<-["transparent","opaque"],do: %{"kind"=>"nominal","name"=>%{"module"=>"other","name"=>"wrapped","arity"=>0,"opacity"=>opacity},"inner"=>inner})
pool=pool++[%{"kind"=>"integers","values"=>[-10,0,1,7,8,"999999999999999999999999"]},%{"kind"=>"range","from"=>-5,"to"=>10},%{"kind"=>"nominal","name"=>%{"module"=>"other","name"=>"finite","arity"=>0,"opacity"=>"opaque"},"inner"=>%{"kind"=>"integers","values"=>[0,7,8]}}]
literals=[:cerl.c_int(0),:cerl.c_int(1),:cerl.c_float(0.0),:cerl.c_float(-0.0),:cerl.c_float(1.0),:cerl.c_atom(true),:cerl.c_atom(false),:cerl.c_atom(:ok),:cerl.c_nil(),:cerl.c_tuple_skel([:cerl.c_int(1)]),:cerl.c_tuple_skel([:cerl.c_float(1.0)]),:cerl.c_cons_skel(:cerl.c_int(1),:cerl.c_nil()),:cerl.c_cons_skel(:cerl.c_float(1.0),:cerl.c_nil()),:cerl.abstract(%{key: 1}),:cerl.abstract(%{key: 1.0}),:cerl.abstract(%{key: -0.0})]
tests=for f<-[:is_atom,:is_boolean,:is_binary,:is_bitstring,:is_float,:is_function,:is_integer,:is_list,:is_map,:is_number,:is_pid,:is_port,:is_reference,:is_tuple],do: {"test_#{f}",call.(f,[x]),pool}
comparisons=for {left,i}<-Enum.with_index(literals),{right,j}<-Enum.with_index(literals),f<-[ :"==",:"=:=",:<,:"=<",:>,:">="],do: {"literal_#{i}_#{j}_#{Atom.to_string(f)}",call.(f,[left,right]),[%{"kind"=>"any"}]}
File.mkdir_p!(".cache/guard_control/call_proof_cores")
reason=fn {kind,details}->
 message=case details do
  [left,op,right]->%{left: List.to_string(left),operator: List.to_string(op),right: List.to_string(right)}
  [function,args]->%{function: Atom.to_string(function),args: List.to_string(args)}
 end
 %{kind: Atom.to_string(kind),message: message}
end
generic=for {f,args} <- [ {:<,[x,y]}, {:>,[x,y]}, {:is_atom,[x,y]}, {:is_function,[x,y,x]}, {:not,[x,y]}, {:abs,[x]}, {:+,[x,y]}, {:-,[x,y]}, {:div,[x,y]}, {:byte_size,[x]}, {:element,[:cerl.c_int(1),x]}, {:hd,[x]}, {:error,[x]}, {:exit,[x]}, {:unknown_guard,[x]}, {:unknown_guard,[]}, {:"=/=",[x,y]}, {:"/=",[x,y]}], do: {"generic_#{f}_#{length(args)}",call.(f,args),pool}
ranges=for f<-[:<,:"=<",:>,:">="],literal<-[:cerl.c_int(7),:cerl.c_int(-999999999999999999999999)],reverse<-[false,true],do: {"range_#{f}_#{:cerl.concrete(literal)}_#{reverse}",call.(f,if(reverse,do: [x,literal],else: [literal,x])),pool}
a=:cerl.c_var(:A);e=:cerl.c_var(:E)
tries=for {name,body,handler} <- [{"test_handler",a,call.(:is_integer,[y])},{"fatal_handler",a,call.(:byte_size,[y])},{"failed_body",call.(:is_integer,[a]),:cerl.c_atom(true)},{"fatal_body",call.(:byte_size,[a]),:cerl.c_atom(true)},{"both_failed",call.(:is_integer,[a]),call.(:is_integer,[y])},{"both_fatal",call.(:byte_size,[a]),call.(:byte_size,[y])}],do: {"try_#{name}",:cerl.c_try(x,[a],body,[e],handler),pool}
for {name,expression,types}<-tests++comparisons++generic++ranges++tries do
 expression=:cerl_trees.map(fn n->:cerl.set_ann(n,[10]) end,expression)
 tree=:cerl.c_module(:cerl.c_atom(:fixture),[:cerl.c_fname(:fixture,2)],[],[{:cerl.c_fname(:fixture,2),:cerl.c_fun([x,y],expression)}]);{tree,_}=:cerl_trees.label(tree,0)
 proof=CoreExport.export_module(:fixture,[],tree);path=Path.expand(".cache/guard_control/call_proof_cores/"<>URI.encode_www_form(name)<>".json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [proof]}))
 [{_,fun}]=:cerl.module_defs(tree);body=:cerl.fun_body(fun)
 for {tx,i}<-Enum.with_index(types),{ty,j}<-Enum.with_index(if(String.starts_with?(name,"generic_") or String.starts_with?(name,"try_"),do: [%{"kind"=>"any"},tx],else: [%{"kind"=>"any"}])),eval<-[:pos,:neg,:dont_know],warnings<-[false,true] do
  map=:cerl_trees.fold(fn node,map->if :cerl.is_c_var(node) and :cerl.var_name(node) in [:X,:Y],do: :rust_binder_native.store_map(:cerl_trees.get_label(node),TypeCodec.type(if(:cerl.var_name(node)==:X,do: tx,else: ty)),map),else: map end,:rust_binder_native.map__new(),tree)
  state=:rust_binder_native.binder_state(:fixture,%{},warnings)
  result=try do
   {map,type,out_state}=:rust_binder_native.bind_guard(body,map,%{},eval,state)
   result=%{map: BinderOracle.map_wire(map,fields.map),value: NativeTypeWire.encode(type)}
   result=if warnings do
    ws=Enum.map(BinderOracle.get(out_state,fields.state,:warnings),fn {:warn_opaque,{file,location,{m,f,a}},{:opaque_guard,details}} ->
     message=case details do
      [left,op,right,positions]->%{left: List.to_string(left),operator: List.to_string(op),right: List.to_string(right),positions: positions}
      [function,args]->%{function: Atom.to_string(function),args: List.to_string(args)}
     end
     %{info: %{file: List.to_string(file),location: location,function: [Atom.to_string(m),Atom.to_string(f),a]},message: message}
    end)
    result |> Map.put(:warnings,ws) |> Map.put(:warning_terms,CoreExport.export_terms(BinderOracle.get(out_state,fields.state,:warnings)))
   else result end
   %{ok: result}
  catch :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}};:throw,{tag,:none} when tag in [:fail,:fatal_fail]->%{failure: %{fatal: tag==:fatal_fail}};:throw,{tag,{_guard,msg}} when tag in [:fail,:fatal_fail]->%{failure: %{fatal: tag==:fatal_fail,reason: reason.(msg)}};c,r->raise "unclassified #{inspect({name,c,r})}"
  after :dialyzer_callgraph.delete(BinderOracle.get(state,fields.state,:callgraph));:dialyzer_codeserver.delete(BinderOracle.get(state,fields.state,:codeserver))end
  row=%{id: "#{name}:#{i}:#{j}:#{eval}:#{warnings}",core_json: path,x: tx,y: ty,eval: Atom.to_string(eval),warnings: warnings}
  IO.puts(:json.encode(Map.merge(row,result)))
 end
end
