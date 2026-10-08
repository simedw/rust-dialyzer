[root,out,base]=System.argv()
for f<-~w(type_codec.exs type_wire.exs core_export.exs),do: Code.require_file(Path.join(root,"scripts/"<>f))
defmodule WholeModuleOracle do
 def signatures(types),do: Map.new(types,fn {label,t}->{Integer.to_string(label),%{arguments: Enum.map(:erl_types.t_fun_args(t),&NativeTypeWire.encode/1),result: NativeTypeWire.encode(:erl_types.t_fun_range(t))}}end)
 def funs(tree),do: :cerl_trees.fold(fn n,acc->if :cerl.is_c_fun(n),do: [{:cerl_trees.get_label(n),:cerl.fun_arity(n)}|acc],else: acc end,[],tree)
end
fixtures=[
 {"local_identity","entry(X) -> helper(X).\nhelper(X) -> X.\nunused(X) -> X."},
 {"local_narrow","entry(X) -> helper(X).\nhelper(X) when is_integer(X) -> X + 1.\nunused(X) -> X."},
 {"case_guard","entry(X) -> case X of {ok, V} when is_integer(V) -> V + 1; [] -> 0; _ -> error(bad) end.\nhelper(X) -> X.\nunused(X) -> X."},
 {"anonymous","entry(X) -> fun(Y) -> helper({X,Y}) end.\nhelper(X) -> X.\nunused(X) -> X."},
 {"discarded_fun","entry(X) -> _F = fun(Y) -> helper({X,Y}) end, ok.\nhelper(X) -> X.\nunused(X) -> X."},
 {"map_exact","entry(X) -> M = \#{a => X}, M\#{missing := 2}.\nhelper(X) -> X.\nunused(X) -> X."},
 {"improper_cons","entry(X) -> [X | 3].\nhelper(X) -> X.\nunused(X) -> X."},
 {"binary","entry(X) -> <<X:8/integer>>.\nhelper(X) -> X.\nunused(X) -> X."},
 {"try","entry(X) -> try helper(X) of {ok,V} -> V catch error:bad -> recovered end.\nhelper(X) -> case X of {ok,_} -> X; _ -> error(bad) end.\nunused(X) -> X."},
 {"recursion","entry(X) -> helper(X).\nhelper(0) -> done; helper(X) when is_integer(X), X > 0 -> helper(X-1).\nunused(X) -> X."},
 {"equality","entry(X) -> case X =:= 1 of true -> one; false -> helper(X) end.\nhelper(X) -> X.\nunused(X) -> X."},
 {"no_return","entry(X) -> helper(X).\nhelper(X) -> error(X).\nunused(X) -> X."}
]
plt=:dialyzer_cplt.from_file(String.to_charlist(base))
try do
 rows=for {id,source}<-fixtures,prior<-~w(absent any none unit integer) do
  name=String.to_atom("whole_module_"<>id);dir=Path.join(Path.dirname(out),"fixtures");File.mkdir_p!(dir);path=Path.join(dir,Atom.to_string(name)<>".erl");File.write!(path,"-module("<>Atom.to_string(name)<>").\n-export([entry/1]).\n"<>source<>"\n")
  {:ok,^name,bin,_}=:compile.file(String.to_charlist(path),[:debug_info,:binary,:return_errors,:return_warnings]);beam=Path.rootname(path)<>".beam";File.write!(beam,bin);exported=CoreExport.module(beam);{_,tree}=CoreExport.rebuild_module(exported|>:json.encode()|>IO.iodata_to_binary()|>:json.decode());{tree,_}=:cerl_trees.label(tree,0)
  cg=:dialyzer_callgraph.new();cs=:dialyzer_codeserver.new()
  try do
   {vertices,edges}=:dialyzer_callgraph.scan_core_tree(tree,cg);:dialyzer_callgraph.add_edges(edges,vertices,cg);cs=:dialyzer_codeserver.insert(name,tree,cs);target=:dialyzer_codeserver.lookup_mod_code(name,cs);info=WholeModuleOracle.funs(target)
   rows=for {label,arity}<-info,prior != "absent" do
    key=case :dialyzer_callgraph.lookup_name(label,cg) do {:ok,mfa}->mfa;:error->label end
    ret=%{"kind"=>prior};args=List.duplicate(%{"kind"=>"any"},arity);:dialyzer_plt.insert_list(plt,[{key,{TypeCodec.type(ret),Enum.map(args,&TypeCodec.type/1)}}]);%{label: label,arguments: args,result: ret}
   end
   {native,warnings}=try do
    types=:dialyzer_dataflow.get_fun_types(target,plt,cg,cs,%{});{warnings,warning_types}=:dialyzer_dataflow.get_warnings(target,plt,cg,cs,%{})
    {%{status: "ok",types: WholeModuleOracle.signatures(types),warnings_match: true,warning_types: WholeModuleOracle.signatures(warning_types),warning_count: length(warnings)},warnings}
   catch kind,reason->{%{status: "error",kind: Atom.to_string(kind),reason: CoreExport.export_terms(reason)},[]}end
   %{id: "#{id}:#{prior}",prior: rows,program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},warnings: CoreExport.export_terms(warnings),native: native}
  after
   for {label,_}<-WholeModuleOracle.funs(tree) do key=case :dialyzer_callgraph.lookup_name(label,cg) do {:ok,mfa}->mfa;:error->label end;:dialyzer_plt.delete_list(plt,[key]) end
   :dialyzer_callgraph.delete(cg);:dialyzer_codeserver.delete(cs)
  end
 end
 File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]));IO.inspect(%{cases: length(rows)})
after :dialyzer_plt.delete(plt)end
