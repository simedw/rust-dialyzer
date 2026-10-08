[root,out]=System.argv()
for f<-~w(type_codec.exs type_wire.exs core_export.exs),do: Code.require_file(Path.join(root,"scripts/"<>f))
src=File.read!(Path.join(root,"scripts/dataflow_state_probes/function_state_oracle.exs"))|>String.split("defmodule FunctionStateOracle")|>List.last()|>String.split(" def run(row")|>hd();Code.eval_string("defmodule FunctionStateOracle"<>src<>"end")
{fields,_}=FunctionStateOracle.load()
get=fn s,k->FunctionStateOracle.get(s,fields,k)end
kind=fn s->%{"kind"=>s}end
ft=fn r->%{"kind"=>"function","domain"=>[],"range"=>r}end
annotations=[[12,{:file,~c"fixture.erl"}],[12,{:file,~c"fixture.erl"},:compiler_generated],[0],[],[{15,4},{:file,~c"other.erl"}]]
returns=[{"any",kind.("any"),"any()"},{"unit",kind.("unit"),"no_return()"},{"function_unit",ft.(kind.("unit")),"fun(() -> no_return())"},{"function_any",ft.(kind.("any")),"fun(() -> any())"},{"none",nil,nil}]
rows=for layout<-~w(return tuple discard nested many shared),bodykind<-~w(normal exit no_match both),{ann,ai}<-Enum.with_index(annotations),{contractid,contractret,contracttext}<-returns,mode<-~w(unhandled called_none called_unit called_any reached_none),warning<-[false,true],baseline<-[false,true] do
 lit=&:cerl.abstract/1;v=&:cerl.c_var/1
 body=case bodykind do "normal"->lit.(:ok);"exit"->:cerl.c_call(lit.(:erlang),lit.(:error),[lit.(:bad)]);"no_match"->:cerl.c_case(lit.(:ok),[]);"both"->:cerl.c_case(v.(:x),[:cerl.c_clause([lit.(:ok)],lit.(true),lit.(:ok)),:cerl.c_clause([v.(:z)],lit.(true),:cerl.c_call(lit.(:erlang),lit.(:error),[lit.(:bad)]))]) end
 anon=:cerl.ann_c_fun(ann,[],body)
 entrybody=case layout do "return"->anon;"tuple"->:cerl.c_tuple_skel([anon,lit.(:ok)]);"discard"->:cerl.c_seq(anon,lit.(:ok));"nested"->:cerl.ann_c_fun(ann,[],anon);"many"->:cerl.c_tuple_skel(for _<-1..40,do: anon);"shared"->shared=v.({:shared,0});:cerl.c_letrec([{shared,anon}],:cerl.c_tuple_skel([shared,:cerl.ann_c_fun(ann,[],:cerl.c_apply(shared,[]))])) end
 prior=:cerl.ann_c_cons([12,{:file,~c"fixture.erl"}],lit.(:head),:cerl.c_seq(lit.(:before),lit.(3)))
 entrybody=:cerl.c_seq(prior,entrybody)
 ev=v.({:entry,1});unused=v.({:unused,1});helper=v.({:helper,1})
 tree=:cerl.c_module(lit.(:warn_fixture),[ev],[],[{ev,:cerl.ann_c_fun(ann,[v.(:x)],entrybody)},{unused,:cerl.ann_c_fun(ann,[v.(:x)],body)},{helper,:cerl.ann_c_fun(ann,[v.(:x)],body)}])
 exported=CoreExport.export_module(:warn_fixture,[],tree);{tree,_}=:cerl_trees.label(tree,0)
 cg=:dialyzer_callgraph.new();plt=:dialyzer_plt.new();cs=:dialyzer_codeserver.new()
 try do
  :ets.insert(elem(cs,2),{{:mod,:warn_fixture},[{~c"fixture.erl",~c"/source/fixture.erl"},{~c"other.erl",~c"/source/other.erl"}]})
  {vertices,edges}=:dialyzer_callgraph.scan_core_tree(tree,cg);:dialyzer_callgraph.add_edges(edges,vertices,cg)
  contract_rows=if contractret do
   source="-spec entry(any()) -> "<>contracttext<>"."
   {:ok,tokens,_}=:erl_scan.string(String.to_charlist(source));{:ok,{:attribute,_,:spec,{_,[form]}}}=:erl_parse.parse_form(tokens)
   nativefun=:erl_types.t_fun([:any],TypeCodec.type(contractret));contract={:contract,[{nativefun,[]}],[:any],[{form,[]}]}
   :dialyzer_plt.insert_contract_list(plt,[{{:warn_fixture,:entry,1},contract}]);[%{mfa: %{module: "warn_fixture",function: "entry",arity: 1},alternatives: [%{function: %{"kind"=>"function","domain"=>[kind.("any")],"range"=>contractret},constraints: []}],capture: CoreExport.export_terms(contract)}]
  else [] end
  state=:rust_function_native.state__new(cg,cs,:cerl.ann_c_fun([{:label,:top}],[],tree),plt,:warn_fixture,%{})
  trees=get.(state,:tree_map)
  result=case mode do "called_unit"->kind.("unit");"called_any"->kind.("any");_->kind.("none") end
  ops=for {label,fun}<-:dict.to_list(trees),label != :top,mode != "unhandled" do %{label: label,arguments: List.duplicate(kind.("any"),length(:cerl.fun_vars(fun))),result: result,reachable: mode=="reached_none"} end
  state=Enum.reduce(ops,state,fn op,s->fun=:dict.fetch(op.label,trees);args=Enum.map(op.arguments,&TypeCodec.type/1);s=:rust_function_native.forward_args(op.label,args,s);s=:rust_function_native.state__update_fun_entry(fun,args,TypeCodec.type(op.result),s);if op.reachable,do: s|>then(&:rust_function_native.state__add_reachable(op.label,&1)),else: s end)
  state=if warning,do: :rust_function_native.state__set_warning_mode(state),else: state
  [{_,entry}|_]=:cerl.module_defs(tree);prior=:cerl.seq_arg(:cerl.fun_body(entry));state=:rust_function_native.state__set_curr_fun(:cerl_trees.get_label(entry),state)
  state=if baseline do {s,_,_}=:rust_function_native.traverse(prior,:rust_function_native.map__new(),state);s else state end
  warns=:rust_function_native.state__get_warnings(state)
  facts=for {label,_}<-:dict.to_list(trees),label != :top,:dialyzer_callgraph.lookup_name(label,cg)==:error do
   incoming=case :dialyzer_callgraph.in_neighbours(label,cg) do :none->[];xs->xs end
   parent_named=case incoming do [parent]->match?({_,_,_},:rust_function_native.state__lookup_name(parent,state));_->false end
   %{escaping: :dialyzer_callgraph.is_escaping(label,cg),incoming: length(incoming),parent_named: parent_named}
  end
  %{id: "#{layout}:#{bodykind}:#{ai}:#{contractid}:#{mode}:#{warning}:#{baseline}",baseline_node: (if baseline,do: :cerl_trees.get_label(prior),else: :null),program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},graph_evidence: facts,operations: ops,contracts: contract_rows,warning_mode: warning,warnings: CoreExport.export_terms(warns),native: %{warning_count: length(warns),warnings_match: true}}
 after :dialyzer_callgraph.delete(cg);:dialyzer_plt.delete(plt);:dialyzer_codeserver.delete(cs)end
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]));IO.inspect(%{cases: length(rows)})
