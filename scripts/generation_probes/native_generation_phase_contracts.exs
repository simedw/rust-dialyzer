[root,out]=System.argv()
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/core_export.exs"))
path=:code.which(:dialyzer_typesig)
{:ok,:dialyzer_typesig,chunks}=:beam_lib.all_chunks(path)
get=fn name->chunks|>List.keyfind(String.to_charlist(name),0)|>elem(1)end
{:ok,{:dialyzer_typesig,[{:atoms,atoms}]}}=:beam_lib.chunks(path,[:atoms])
atoms=Map.new(atoms)
<<fun_count::32,fun_entries::binary>>=get.("FunT")
fun_table=for <<name::32,arity::32,label::32,index::32,num_free::32,uniq::32<-fun_entries>>,do: %{name: Atom.to_string(Map.fetch!(atoms,name)),arity: arity,label: label,index: index,env_count: num_free,uniq: uniq}
true=length(fun_table)==fun_count
fun_table=Enum.with_index(fun_table)|>Enum.map(fn {entry,new_index}->Map.put(entry,:new_index,new_index)end)
read_entries=fn <<count::32,rest::binary>>->entries=for <<a::32,arity::32,label::32<-rest>>,do: {a,arity,label};true=count==length(entries);entries end
exports=read_entries.(get.("ExpT"));locals=read_entries.(get.("LocT"))
entries=Enum.uniq(exports++locals)|>Enum.sort()
new_exp=<<length(entries)::32>><>IO.iodata_to_binary(Enum.map(entries,fn {a,b,c}-><<a::32,b::32,c::32>>end))
{:ok,patched}=:beam_lib.build_module(List.keyreplace(chunks,~c"ExpT",0,{~c"ExpT",new_exp}))
{:ok,:dialyzer_typesig,patched_chunks}=:beam_lib.all_chunks(patched)
true=Enum.all?(chunks,fn {name,data}->name==~c"ExpT" or List.keyfind(patched_chunks,name,0)=={name,data}end)
:code.purge(:dialyzer_typesig)
{:module,:dialyzer_typesig}=:code.load_binary(:dialyzer_typesig,path,patched)
# Relative synthetic labels are proof normalization only. The native allocator remains unchanged.
relative=fn recur,term ->
 case term do
  {:c,:var,label,:unknown} -> {:c,:var,Map.get(Process.get(:synthetic_labels,%{}),label,label),:unknown}
  xs when is_tuple(xs)->xs|>Tuple.to_list()|>Enum.map(&recur.(recur,&1))|>List.to_tuple()
  [head|tail]->[recur.(recur,head)|recur.(recur,tail)]
  xs when is_map(xs)->Map.new(xs,fn {k,v}->{recur.(recur,k),recur.(recur,v)}end)
  _->term
 end
end
{:module,:dialyzer_utils}=:code.ensure_loaded(:dialyzer_utils)
:erlang.trace_pattern({:dialyzer_utils,:label,1},[{:_,[],[{:return_trace}]}],[:local])
trace_allocations=fn function ->
 parent=self()
 tracer=spawn(fn ->
  loop=fn loop,labels ->
   receive do
    {:trace,_,:call,{:dialyzer_utils,:label,[_]}}->loop.(loop,labels)
    {:trace,_,:return_from,{:dialyzer_utils,:label,1},node}->loop.(loop,[:cerl_trees.get_label(node)|labels])
    {:result,ref}->send(parent,{ref,Enum.reverse(labels)})
   end
  end
  loop.(loop,[])
 end)
 :erlang.trace(self(),true,[:call,{:tracer,tracer}])
 result=function.()
 ref=:erlang.trace_delivered(self())
 receive do {:trace_delivered,_,^ref}->:ok end
 :erlang.trace(self(),false,[:call])
 reply=make_ref();send(tracer,{:result,reply})
 labels=receive do {^reply,n}->n end
 true=Enum.all?(Enum.chunk_every(labels,2,1,:discard),fn [a,b]->a>b end)
 Process.put(:synthetic_labels,labels|>Enum.with_index(1)|>Map.new(fn {label,index}->{label,-index}end))
 {result,length(labels)}
end
wire_id=fn v->if is_atom(v),do: %{atom: Atom.to_string(v)},else: %{integer: Integer.to_string(Map.get(Process.get(:synthetic_labels,%{}),v,v))}end
encode=fn recur,node,{nodes,count}->
 case node do
  {:constraint,lhs,relation,rhs,deps}->
    right=if match?({:fun_var,_,_,_},rhs) do
      {:fun_var,fun,_,_}=rhs;info=Map.new(:erlang.fun_info(fun))
      %{derived: true,closure: %{kind: "local",module: Atom.to_string(info.module),name: Atom.to_string(info.name),new_index: info.new_index,uniq: info.uniq,arity: info.arity,env_count: length(info.env),env_terms: CoreExport.export_terms(relative.(relative,info.env))}}
    else
      NativeTypeWire.encode(relative.(relative,rhs))
    end
    {[%{kind: "atomic",left: NativeTypeWire.encode(relative.(relative,lhs)),relation: Atom.to_string(relation),right: right,deps: Enum.map(deps,wire_id)}|nodes],count+1,count}
  {:constraint_ref,function,deps}->{[%{kind: "ref",function: wire_id.(:erl_types.t_var_name(function)),deps: Enum.map(deps,wire_id)}|nodes],count+1,count}
  {:constraint_list,kind,children,deps,_,_}->
   {ids,{nodes,count}}=Enum.map_reduce(children,{nodes,count},fn child,state->{ns,nc,id}=recur.(recur,child,state);{id,{ns,nc}}end)
   {[%{kind: Atom.to_string(kind),children: ids,deps: Enum.map(deps,wire_id)}|nodes],count+1,count}
 end
end
lit=&:cerl.abstract/1
v=&:cerl.c_var/1
x=v.(:x);y=v.(:y);z=v.(:z)
fail=:cerl.c_primop(lit.(:match_fail),[])
h1=v.({:helper1,1});h2=v.({:helper2,1})
choose=:cerl.c_case(x,[:cerl.c_clause([lit.(:true)],lit.(:true),h1),:cerl.c_clause([lit.(:false)],lit.(:true),h2)])
choose_mixed=:cerl.c_case(x,[:cerl.c_clause([lit.(:true)],lit.(:true),h1),:cerl.c_clause([lit.(:false)],lit.(:true),:cerl.c_fun([y],y))])
choose_external=:cerl.c_case(x,[:cerl.c_clause([lit.(:true)],lit.(:true),h1),:cerl.c_clause([lit.(:false)],lit.(:true),y)])
ops=[h1,choose,:cerl.c_let([y],choose,y),choose_mixed,choose_external]
sigs=[{:erl_types.t_any(),:erl_types.t_atom(:a)},{:erl_types.t_atom(),:erl_types.t_atom(:b)},{:erl_types.t_integer(),:erl_types.t_integer()},{:erl_types.t_none(),:erl_types.t_none()},{:erl_types.t_any(),:erl_types.t_none()}]
varx=:erl_types.t_var(:X)
contracts=[nil,[{:erl_types.t_fun([:erl_types.t_any()],:erl_types.t_atom(:ok)),[]}],[{:erl_types.t_fun([:erl_types.t_atom()],:erl_types.t_atom(:tag)),[]},{:erl_types.t_fun([:erl_types.t_integer()],:erl_types.t_integer()),[]}],[{:erl_types.t_fun([varx],varx),[{:subtype,varx,:erl_types.t_integer()}]}],[{:erl_types.t_fun([varx],:erl_types.t_tuple([:erl_types.t_atom(:tag),varx])),[{:subtype,varx,:erl_types.t_list()}]}]]
base=for op<-ops,arg<-[z,lit.(:a),lit.(1)],sig1<-[hd(sigs),Enum.at(sigs,2),List.last(sigs)],c1<-contracts,c2<-contracts,do: {:cerl.c_apply(op,[arg]),sig1,Enum.at(sigs,1),c1,c2,true}
static=for arg<-[z,lit.(:a),lit.(1)],sig1<-sigs,c1<-contracts,present<-[true,false],do: {:cerl.c_call(lit.(:owner),lit.(:helper1),[arg]),sig1,hd(sigs),c1,nil,present}
base=base++static
base=base++Enum.map(base,fn {body,a,b,c,d,e}->{:cerl.c_fun([],body),a,b,c,d,e}end)
prepare_contract=fn alternatives ->
 if alternatives==nil do nil else
  general=alternatives|>Enum.map(fn {signature,constraints}->:erl_types.t_subst(signature,Map.new(constraints,fn {:subtype,l,r}->{:erl_types.t_var_name(l),r}end))end)|>:erl_types.t_sup()|>:erl_types.t_subst(%{})
  {:contract,alternatives,:erl_types.t_fun_args(general),[]}
 end
end
profile=%{module: "dialyzer_typesig",original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path)))),fun_table: fun_table}
rows=base|>Enum.with_index()|>Enum.map(fn {{body,sig1,sig2,c1,c2,present},i}->
 name=String.to_atom("synthetic#{i}")
 body=:cerl_trees.map(fn node->if :cerl.is_c_atom(node) and :cerl.atom_val(node)==:owner,do: lit.(name),else: node end,body)
 variable=v.({:entry,3});fun=:cerl.c_fun([x,y,z],body)
 module=:cerl.c_module(lit.(name),[variable,h1,h2],[],[{variable,fun},{h1,:cerl.c_fun([z],z)},{h2,:cerl.c_fun([z],z)}])
 exported=CoreExport.export_module(name,[],module)
 {module,next}=:cerl_trees.label(module,0)
 [{variable,fun}|_]=:cerl.module_defs(module)
 plt=:dialyzer_plt.new()
 cg=:dialyzer_callgraph.new()
 :dialyzer_callgraph.scan_core_tree(module,cg)
 {arg1,ret1}=sig1;{arg2,ret2}=sig2
 if present,do: :dialyzer_plt.insert_list(plt,[{{name,:helper1,1},{ret1,[arg1]}},{{name,:helper2,1},{ret2,[arg2]}}])
 prior_contracts=Enum.flat_map([{:helper1,prepare_contract.(c1)},{:helper2,prepare_contract.(c2)}],fn {f,contract}->
  if contract==nil do [] else
   :dialyzer_plt.insert_contract_list(plt,[{{name,f,1},contract}])
   {:contract,alternatives,arguments,_}=contract
   [%{mfa: %{module: Atom.to_string(name),function: Atom.to_string(f),arity: 1},arguments: Enum.map(arguments,&NativeTypeWire.encode/1),alternatives: Enum.map(alternatives,fn {signature,constraints}->%{signature: NativeTypeWire.encode(signature),constraints: Enum.map(constraints,fn {:subtype,l,r}->[NativeTypeWire.encode(l),NativeTypeWire.encode(r)]end)}end),captured_record: CoreExport.export_terms([contract])}]
  end
 end)
 signatures=Enum.map(if(present,do: [{:helper1,arg1,ret1},{:helper2,arg2,ret2}],else: []),fn {f,arg,ret}->%{mfa: %{module: Atom.to_string(name),function: Atom.to_string(f),arity: 1},arguments: [NativeTypeWire.encode(arg)],return: NativeTypeWire.encode(ret)}end)
 state={:state,cg,:undefined,[],%{},%{},%{},false,false,name,%{},next,false,plt,%{},[{name,%{}}],[],[{name,:entry,3}],[:v2]}
 state=:dialyzer_typesig.state__store_funs([variable],[fun],state)
 {{state,_},allocations}=trace_allocations.(fn -> :dialyzer_typesig.traverse(fun,:sets.from_list([:cerl_trees.get_label(variable)]),state) end)
 functions=elem(state,4)|>Enum.sort()|>Enum.map(fn {f,body}->{flat,_,root_id}=encode.(encode,body,{[],0});%{function: wire_id.(:erl_types.t_var_name(f)),graph: %{root: root_id,nodes: Enum.reverse(flat)}}end)
 next_label=elem(state,11)
 solved=:dialyzer_typesig.solve(:dialyzer_typesig.state__scc(state),:dialyzer_typesig.state__finalize(state))
 :dialyzer_plt.delete(plt)
 :dialyzer_callgraph.delete(cg)
 bindings=solved|>Enum.sort()|>Enum.map(fn {label,type}->%{id: wire_id.(label),type: NativeTypeWire.encode(relative.(relative,type))}end)
 %{indexed_callgraph: true,entry_names: ["entry"],prior_signatures: signatures,prior_contracts: prior_contracts,program: %{version: 5,profile: CoreExport.profile(),modules: [exported]},profile: profile,native: %{status: "generated",functions: functions,next_label: next_label,bindings: bindings,refold_allocations: allocations}}
end)
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows),original_beam_profile: profile.original_sha256})
