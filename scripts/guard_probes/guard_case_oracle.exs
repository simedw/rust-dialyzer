Code.require_file("../core_export.exs",__DIR__)
Code.require_file("../type_codec.exs",__DIR__)
Code.require_file("../type_wire.exs",__DIR__)
source=File.read!(Path.join(__DIR__,"../pattern_probes/binder_oracle.exs"))|>String.split("fields=BinderOracle.load()")|>hd()|>String.split("defmodule BinderOracle")|>List.last()|>String.split(" def run")|>hd()
Code.eval_string("defmodule BinderOracle"<>source<>"end")
fields=BinderOracle.load()

File.mkdir_p!(".cache/guard_full/cases_proof_cores")
x=:cerl.c_var(:X);y=:cerl.c_var(:Y)
call=fn f,args -> :cerl.c_call(:cerl.c_atom(:erlang),:cerl.c_atom(f),args) end
pool=Enum.map(["any","none","unit","integer","atom","binary","nil","map_any"],&%{"kind"=>&1})++[%{"kind"=>"atoms","values"=>["true"]},%{"kind"=>"atoms","values"=>["false"]},%{"kind"=>"atoms","values"=>["true","false"]},%{"kind"=>"atoms","values"=>["ok"]},%{"kind"=>"var","id"=>%{"atom"=>"A"}},%{"kind"=>"opaque"}]++(for inner<-[%{"kind"=>"integer"},%{"kind"=>"atoms","values"=>["true"]},%{"kind"=>"map_any"}],opacity<-["transparent","opaque"],do: %{"kind"=>"nominal","name"=>%{"module"=>"other","name"=>"wrapped","arity"=>0,"opacity"=>opacity},"inner"=>inner})
pool=pool++[%{"kind"=>"integers","values"=>[-10,0,1,7,8,"999999999999999999999999"]},%{"kind"=>"range","from"=>-5,"to"=>10},%{"kind"=>"nominal","name"=>%{"module"=>"other","name"=>"finite","arity"=>0,"opacity"=>"opaque"},"inner"=>%{"kind"=>"integers","values"=>[0,7,8]}}]
File.mkdir_p!("data/call_proof_cores")
reason=fn {kind,details}->
 message=case details do
  [actual,tag] when kind==:record_matching->%{actual: List.to_string(actual),tag: Atom.to_string(tag)}
  [left,op,right]->%{left: List.to_string(left),operator: List.to_string(op),right: List.to_string(right)}
  [function,args]->%{function: Atom.to_string(function),args: List.to_string(args)}
 end
 %{kind: Atom.to_string(kind),message: message}
end
z=:cerl.c_var(:Z)
clause=fn ps,g,b->:cerl.c_clause(ps,g,b) end
lit=&:cerl.c_atom/1
case_expr=fn arg,clauses->:cerl.c_case(arg,clauses) end
shapes=[
 {"case_bools",case_expr.(x,[clause.([lit.(true)],lit.(true),lit.(true)),clause.([lit.(false)],lit.(true),lit.(false))]),pool},
 {"case_bools_reverse",case_expr.(x,[clause.([lit.(false)],lit.(true),lit.(false)),clause.([lit.(true)],lit.(true),lit.(true))]),pool},
 {"case_type_test",case_expr.(call.(:is_integer,[x]),[clause.([lit.(true)],lit.(true),call.(:is_atom,[y])),clause.([lit.(false)],lit.(true),call.(:is_list,[y]))]),pool},
 {"case_var",case_expr.(x,[clause.([z],call.(:is_integer,[z]),lit.(true)),clause.([z],lit.(true),lit.(false))]),pool},
 {"case_tuple",case_expr.(x,[clause.([:cerl.c_tuple_skel([lit.(:tag),z])],lit.(true),call.(:is_integer,[z])),clause.([y],lit.(true),lit.(false))]),pool},
 {"case_list",case_expr.(x,[clause.([:cerl.c_cons_skel(z,:cerl.c_nil())],lit.(true),call.(:is_atom,[z])),clause.([y],lit.(true),lit.(false))]),pool},
 {"case_values",case_expr.(:cerl.c_values([x,y]),[clause.([z,:cerl.c_int(7)],lit.(true),call.(:is_integer,[z]))]),pool},
 {"case_filter_literal",case_expr.(:cerl.c_values([]),[clause.([],lit.(true),lit.(:fail)),clause.([],lit.(true),lit.(true))]),pool},
 {"case_filter_primop",case_expr.(:cerl.c_values([]),[clause.([],lit.(true),:cerl.c_primop(lit.(:match_fail),[x])),clause.([],lit.(true),lit.(false))]),pool},
 {"case_false_guard",case_expr.(x,[clause.([z],lit.(false),call.(:is_atom,[x])),clause.([z],lit.(true),call.(:is_integer,[z]))]),pool},
 {"case_body_fail",case_expr.(x,[clause.([z],lit.(true),call.(:"=:=",[:cerl.c_int(1),:cerl.c_int(2)])),clause.([y],lit.(true),lit.(true))]),pool},
 {"case_nested",case_expr.(x,[clause.([z],lit.(true),case_expr.(y,[clause.([lit.(true)],lit.(true),call.(:is_integer,[z])),clause.([lit.(false)],lit.(true),lit.(false))]))]),pool}
]
records_options=[[]]
for {{name,expression,types},expression_id}<-Enum.with_index(shapes) do
 expression=:cerl_trees.map(fn n->:cerl.set_ann(n,[10]) end,expression)
 tree=:cerl.c_module(:cerl.c_atom(:fixture),[:cerl.c_fname(:fixture,2)],[],[{:cerl.c_fname(:fixture,2),:cerl.c_fun([x,y],expression)}]);{tree,_}=:cerl_trees.label(tree,0)
 proof=CoreExport.export_module(:fixture,[],tree);path=Path.expand(".cache/guard_full/cases_proof_cores/"<>"case_#{expression_id}.json");File.write!(path,:json.encode(%{version: 6,profile: CoreExport.profile(),modules: [proof]}))
 [{_,fun}]=:cerl.module_defs(tree);body=:cerl.fun_body(fun)
 for {tx,i}<-Enum.with_index(types),{ty,j}<-Enum.with_index(pool),not(name=="function_vars" and ty["kind"]=="unit"),eval<-[:pos,:neg,:dont_know],warnings<-[false,true],{records,record_index}<-Enum.with_index(if(String.starts_with?(name,"record"),do: records_options,else: [[]])) do
  map=:cerl_trees.fold(fn node,map->if :cerl.is_c_var(node) and :cerl.var_name(node) in [:X,:Y],do: :rust_binder_native.store_map(:cerl_trees.get_label(node),TypeCodec.type(if(:cerl.var_name(node)==:X,do: tx,else: ty)),map),else: map end,:rust_binder_native.map__new(),tree)
  record_table=Map.new(records,fn record->fs=Enum.with_index(record["fields"],fn t,i->{String.to_atom("field#{i}"),{:type,0,:any,[]},TypeCodec.type(t)} end);{{:record,String.to_atom(record["tag"])},{{~c"fixture",1},[{length(fs),fs}]}} end)
  state=:rust_binder_native.binder_state(:fixture,record_table,warnings)
  result=try do
   {map,type,out_state}=:rust_binder_native.bind_guard(body,map,%{},eval,state)
   result=%{map: BinderOracle.map_wire(map,fields.map),value: NativeTypeWire.encode(type)}
   result=if warnings do
    ws=Enum.map(BinderOracle.get(out_state,fields.state,:warnings),fn {:warn_opaque,{file,location,{m,f,a}},{warning_kind,details}} ->
     message=case {warning_kind,details} do
      {:opaque_match,parts}->%{match: Enum.map(parts,&List.to_string/1)}
      {:opaque_guard,[left,op,right,positions]}->%{left: List.to_string(left),operator: List.to_string(op),right: List.to_string(right),positions: positions}
      {:opaque_guard,[function,args]}->%{function: Atom.to_string(function),args: List.to_string(args)}
     end
     %{info: %{file: List.to_string(file),location: location,function: [Atom.to_string(m),Atom.to_string(f),a]},message: message}
    end)
    result |> Map.put(:warnings,ws) |> Map.put(:warning_terms,CoreExport.export_terms(BinderOracle.get(out_state,fields.state,:warnings)))
   else result end
   %{ok: result}
  catch :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}};:error,{:badmatch,type}->%{error: %{kind: "error",class: "badmatch",payload: %{kind: "type",value: NativeTypeWire.encode(type)}}};:throw,{tag,:none} when tag in [:fail,:fatal_fail]->%{failure: %{fatal: tag==:fatal_fail}};:throw,{tag,{_guard,msg}} when tag in [:fail,:fatal_fail]->%{failure: %{fatal: tag==:fatal_fail,reason: reason.(msg)}};c,r->raise "unclassified #{inspect({name,c,r})}"
  after :dialyzer_callgraph.delete(BinderOracle.get(state,fields.state,:callgraph));:dialyzer_codeserver.delete(BinderOracle.get(state,fields.state,:codeserver))end
  row=%{records: records,id: "#{name}:#{i}:#{j}:#{eval}:#{warnings}:#{record_index}",core_json: path,x: tx,y: ty,eval: Atom.to_string(eval),warnings: warnings}
  IO.puts(:json.encode(Map.merge(row,result)))
 end
end
