[root,out]=System.argv()
for f<-~w(type_codec.exs type_wire.exs core_export.exs),do: Code.require_file(Path.join(root,"scripts/"<>f))
defmodule :core_loop_fixture do
 def type_sig(arity,ret),do: :erl_types.t_fun(List.duplicate(if(Process.get(:fail_args),do: :none,else: :any),arity),TypeCodec.type(ret))
 def output(info,ret),do: Enum.map(info,fn {label,arity}->{label,type_sig(arity,ret)}end)
 def parallel_job(kind,jobs,init,_) do
  Enum.flat_map(jobs,fn job->case kind do
   :typesig->{_,scc}=job;:core_loop_native.find_succ_types_for_scc(scc,init)
   :dataflow->:core_loop_native.refine_one_module(job,init)
  end end)
 end
 def analyze_scc(mfas,label,cg,cs,plt,previous,_) do
  index=Process.get(:scc_index,0);Process.put(:scc_index,index+1);ret=Enum.at(Process.get(:scc_returns),index)||List.last(Process.get(:scc_returns))
  info=Enum.flat_map(mfas,fn mfa->{_,fun}=:dialyzer_codeserver.lookup_mfa_code(mfa,cs);:core_loop_native.collect_fun_info(fun)end)
  active=for {name,_}<-Process.get(:contracts),:dialyzer_plt.is_contract(plt,name),do: namewire(name)
  event=%{kind: "scc",mfas: Enum.map(mfas,fn {m,f,a}->%{module: Atom.to_string(m),function: Atom.to_string(f),arity: a}end),next_label: label,previous: sigmap(previous),active: active}
  info=if Process.get(:named_only),do: Enum.filter(info,fn {label,_}->:dialyzer_callgraph.lookup_name(label,cg)!=:error end),else: info
  log(event);:orddict.from_list(output(info,ret)++[{999999,type_sig(0,%{"kind"=>"integer"})}])
 end
 def get_fun_types(tree,plt,cg,_,_) do
  index=Process.get(:module_index,0);Process.put(:module_index,index+1);ret=Enum.at(Process.get(:module_returns),index)||List.last(Process.get(:module_returns))||%{"kind"=>"any"}
  info=:core_loop_native.collect_fun_info(tree);previous=:core_loop_native.get_fun_types_from_plt(info,cg,plt)
  active=for {name,_}<-Process.get(:contracts),:dialyzer_plt.is_contract(plt,name),do: namewire(name)
  log(%{kind: "module",module: "fixture",previous: sigmap(previous),active: active});output(info,ret)|>:orddict.from_list()
 end
 def module_postorder_from_funs(labels,cg) do log(%{kind: "modules",changed: labels});{[:fixture],cg}end
 def reset_from_funs(labels,cg) do log(%{kind: "reset",changed: labels});{Process.get(:sccs),cg}end
 def start_stamp(_,_),do: :ok
 def end_stamp(_),do: :ok
 def log(e),do: Process.put(:events,[e|Process.get(:events,[])])
 def namewire({m,f,a}),do: %{module: Atom.to_string(m),function: Atom.to_string(f),arity: a}
 def sigmap(types),do: Map.new(types,fn {label,t}->{Integer.to_string(label),%{arguments: Enum.map(:erl_types.t_fun_args(t),&NativeTypeWire.encode/1),result: NativeTypeWire.encode(:erl_types.t_fun_range(t))}}end)
end
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_succ_typings),[:abstract_code])
rewrite=fn walk,x->
 x=case x do
  {:attribute,a,:module,_}->{:attribute,a,:module,:core_loop_native}
  {:remote,a,{:atom,b,module},{:atom,c,name}} when module in [:dialyzer_typesig,:dialyzer_dataflow,:dialyzer_coordinator,:dialyzer_timing]->{:remote,a,{:atom,b,:core_loop_fixture},{:atom,c,name}}
  {:remote,a,{:atom,b,:dialyzer_callgraph},{:atom,c,name}} when name in [:module_postorder_from_funs,:reset_from_funs]->{:remote,a,{:atom,b,:core_loop_fixture},{:atom,c,name}}
  x->x
 end
 cond do is_tuple(x)->x|>Tuple.to_list()|>Enum.map(&walk.(walk,&1))|>List.to_tuple();is_list(x)->Enum.map(x,&walk.(walk,&1));true->x end
end
forms=Enum.map(forms,&rewrite.(rewrite,&1))
{:ok,tokens,_}=:erl_scan.string(~c"fixture_state(CG,CS,Plt) -> #st{callgraph=CG,codeserver=CS,plt=Plt,timing_server=undefined,solvers=[]}.");{:ok,wrapper}=:erl_parse.parse_form(tokens)
{:ok,:core_loop_native,bin,_}=:compile.forms(forms++[wrapper],[:binary,:export_all,:return_warnings]);{:module,:core_loop_native}=:code.load_binary(:core_loop_native,~c"source_loop",bin)
k=fn s->%{"kind"=>s}end;ok=%{"kind"=>"atoms","values"=>["ok"]}
deep=fn depth->Enum.reduce(1..depth,k.("integer"),fn _,inner->%{"kind"=>"tuple","elements"=>[inner]}end)end
plans=[{"failed_args",[ok],[k.("any")]},{"limited_equal",[deep.(7)],[deep.(10)]},{"strict_stop",[k.("any")],[]},{"failed_stop",[k.("none")],[]},{"module_equal",[ok],[ok]},{"module_wider",[ok],[k.("any")]},{"module_failed",[ok],[k.("none")]},{"reset",[ok,k.("integer")],[k.("integer")]},{"unit_then_failed",[k.("unit")],[k.("none")]}]
rows=for {plan,scc_returns,module_returns}<-plans,nested<-[0,1,40],contractret<-[nil,k.("any"),k.("unit"),ok],named_only<-[false,true] do
 lit=&:cerl.abstract/1;v=&:cerl.c_var/1;anon=:cerl.ann_c_fun([1],[],lit.(:ok));body=if nested==0,do: lit.(:ok),else: :cerl.c_tuple_skel(for _<-1..nested,do: anon)
 var=v.({:entry,1});tree=:cerl.c_module(lit.(:fixture),[var],[],[{var,:cerl.ann_c_fun([1],[v.(:x)],body)}]);exported=CoreExport.export_module(:fixture,[],tree);{tree,_}=:cerl_trees.label(tree,0);cg=:dialyzer_callgraph.new();plt=:dialyzer_plt.new();cs=:dialyzer_codeserver.new()
 try do
  {vertices,edges}=:dialyzer_callgraph.scan_core_tree(tree,cg);:dialyzer_callgraph.add_edges(edges,vertices,cg);cs=:dialyzer_codeserver.insert(:fixture,tree,cs);cs=:dialyzer_codeserver.set_next_core_label(1000000,cs)
  contracts=if contractret do
   fun=:erl_types.t_fun([:any],TypeCodec.type(contractret));contract={:contract,[{fun,[]}],[:any],[]};:dialyzer_codeserver.store_contracts(:fixture,%{{:fixture,:entry,1}=>{{~c"fixture.erl",1},contract,[]}},%{},cs);[{{:fixture,:entry,1},contract}]
  else [] end
  info=:core_loop_native.collect_fun_info(tree);sccs=[{1,[{:fixture,:entry,1}]}];Process.put(:fail_args,plan=="failed_args");Process.put(:named_only,named_only);Process.put(:contracts,contracts);Process.put(:sccs,sccs);Process.put(:scc_returns,scc_returns);Process.put(:module_returns,module_returns);Process.put(:scc_index,0);Process.put(:module_index,0);Process.put(:events,[])
  {native,events}=try do
   state=:core_loop_native.fixture_state(cg,cs,plt);:core_loop_native.get_refined_success_typings(sccs,state)
   signatures=for {label,_arity}<-info,{:value,{ret,args}}<-[:dialyzer_plt.lookup(plt,:core_loop_native.lookup_name(label,cg))],do: {label,:erl_types.t_fun(args,ret)}
   signatures=:core_loop_fixture.sigmap(signatures);active=for {name,_}<-contracts,:dialyzer_plt.is_contract(plt,name),do: :core_loop_fixture.namewire(name)
   {%{status: "ok",signatures: signatures,active: active},Enum.reverse(Process.get(:events))}
  catch kind,reason->{%{status: "error",kind: Atom.to_string(kind),reason: CoreExport.export_terms(reason)},Enum.reverse(Process.get(:events))}end
  %{id: "#{plan}:#{nested}:#{inspect(contractret)}:#{named_only}",named_only: named_only,fail_args: plan=="failed_args",program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},next_label: 1000000,scc_returns: scc_returns,module_returns: module_returns,contract_return: contractret||:null,native: native,events: events}
 after :dialyzer_callgraph.delete(cg);:dialyzer_plt.delete(plt);:dialyzer_codeserver.delete(cs)end
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]));IO.inspect(%{cases: length(rows)})
