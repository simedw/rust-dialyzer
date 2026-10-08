[root,out]=System.argv()
true=Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:dialyzer_typesig)))))=="08945A69DD63A44CE877D696E1C27BE2753DE29AE6FC1F8E183D37AB0ADD9882"
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/core_export.exs"))
lit=&:cerl.abstract/1
v=&:cerl.c_var/1
x=v.(:x);y=v.(:y);z=v.(:z)
call=fn f,args->:cerl.c_call(lit.(:erlang),lit.(f),args)end
leaves=[:true,:false,:a,0,1,-1,1.0,[],{},[1],[1|:tail],%{},%{a: 1},%{1=>:a},%{1.0=>:float},<<1>>,<<3::3>>]
patterns=[x,:cerl.c_tuple_skel([x,y]),:cerl.c_cons_skel(x,y),:cerl.c_cons_skel(x,lit.([])),:cerl.c_alias(z,:cerl.c_tuple_skel([x,y])),:cerl.c_binary([])]++Enum.map(leaves,lit)++Enum.map(leaves,&:cerl.c_tuple_skel([x,lit.(&1)]))++Enum.map(leaves,&lit.({&1,%{a: &1}}))
patterns=patterns++[:cerl.c_map_pattern([:cerl.c_map_pair_exact(lit.(:a),x)]),:cerl.c_map_pattern([:cerl.c_map_pair_exact(lit.(:a),x),:cerl.c_map_pair_exact(lit.(:a),y)]),:cerl.c_map_pattern([:cerl.c_map_pair_exact(x,y)]),:cerl.c_map_pattern([:cerl.c_map_pair_exact(:cerl.c_tuple_skel([lit.(:a)]),x)]),:cerl.c_map_pattern([:cerl.c_map_pair_exact(lit.(:a),lit.(:a)),:cerl.c_map_pair_exact(lit.(:a),lit.(:b))])]
malformed_pairs=[:cerl.c_map_pair_exact(lit.(:a),y)]
patterns=patterns++Enum.map([x,lit.(%{a: 1})],fn base->:cerl.update_c_map(:cerl.c_map_pattern(malformed_pairs),base,malformed_pairs)end)
tests=[:is_atom,:is_boolean,:is_binary,:is_bitstring,:is_float,:is_function,:is_integer,:is_list,:is_map,:is_number,:is_pid,:is_port,:is_reference,:is_tuple]
guards=[lit.(:true),lit.(:false),x,y,z]++Enum.map(tests,&call.(&1,[x]))++Enum.map([-1,0,1,2,255,256,1_000_000_000_000_000_000_000_000_000,-1_000_000_000_000_000_000_000_000_000,:unknown],&call.(:is_function,[x,lit.(&1)]))++[call.(:and,[x,y]),call.(:and,[lit.(:true),lit.(:true)]),call.(:and,[lit.(:true),lit.(:false)]),call.(:or,[x,y]),call.(:"=:=",[x,y]),call.(:==,[x,y]),call.(:is_list,[lit.([])]),call.(:is_tuple,[lit.({})]),call.(:is_atom,[lit.(:a)]),call.(:is_integer,[lit.(1)]),call.(:is_integer,[lit.(1.0)]),call.(:is_atom,[:cerl.c_tuple_skel([lit.(:a)])])]
rows=for {pat,i}<-Enum.with_index(patterns),{guard,j}<-Enum.with_index(guards) do
 name=String.to_atom("under#{i}_#{j}");variable=v.({:entry,3});clause=:cerl.c_clause([pat],guard,lit.(:ok));fun=:cerl.c_fun([x,y,z],:cerl.c_case(x,[clause]));module=:cerl.c_module(lit.(name),[variable],[],[{variable,fun}]);exported=CoreExport.export_module(name,[],module)
 {labeled,_}=:cerl_trees.label(module,0);[{_,labeled_fun}]=:cerl.module_defs(labeled);[clause]=:cerl.case_clauses(:cerl.fun_body(labeled_fun))
 labeled_patterns=:cerl.clause_pats(clause)
 # The labeler reconstructs map patterns with an empty arg. Restore deliberately malformed args so this tests the public helper's own assertion and variable traversal.
 labeled_patterns=case {:cerl.type(pat),labeled_patterns} do
  {:map,[labeled_pat]}->[:cerl.update_c_map(labeled_pat,:cerl.map_arg(pat),:cerl.map_es(labeled_pat))]
  _->labeled_patterns
 end
 native=try do
  %{type: NativeTypeWire.encode(:dialyzer_typesig.get_safe_underapprox(labeled_patterns,:cerl.clause_guard(clause)))}
 catch
  :error,:function_clause->%{error: %{kind: "error",class: "function_clause"}}
  :error,:badarg->%{error: %{kind: "error",class: "badarg"}}
  :error,{:badmatch,b} when is_boolean(b)->%{error: %{kind: "error",class: "badmatch",payload: %{kind: "boolean",value: b}}}
 end
 %{program: %{version: 5,profile: CoreExport.profile(),modules: [exported]},native: native}
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows),source_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(:code.which(:dialyzer_typesig)))))})
