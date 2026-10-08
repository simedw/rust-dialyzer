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
base_plt=Path.join(root,".cache/corpus/otp/base.plt")
shared_plt=:dialyzer_cplt.from_file(String.to_charlist(base_plt))
mfas=[{:erlang,:get_module_info,1},{:erlang,:get_module_info,2},{:erlang,:iolist_to_binary,1},{:lists,:map,2},{:lists,:foldl,3},{:lists,:filter,2},{:lists,:reverse,1},{:lists,:append,1},{:lists,:sort,1},{:lists,:keyfind,3},{:maps,:get,2},{:maps,:find,2},{:maps,:keys,1},{:string,:length,1},{:string,:split,2},{:binary,:part,2},{:unicode,:characters_to_binary,1},{:io_lib,:format,2}]
base=Enum.flat_map(mfas,fn {m,f,a}=mfa->
 args=case a do
  1->[[x],[lit.(:a)],[lit.(1)],[lit.([])],[:cerl.c_tuple_skel([lit.(:tag),x])]]
  2->[[x,y],[lit.(:a),x],[lit.(1),lit.(2)],[:cerl.c_fun([z],:cerl.c_tuple_skel([lit.(:tag),z])),:cerl.c_cons_skel(x,lit.([]))],[lit.(:erlang),lit.(:exports)]]
  3->[[x,y,z],[lit.(:a),lit.(1),:cerl.c_cons_skel(x,lit.([]))],[:cerl.c_fun([y,z],:cerl.c_tuple_skel([y,z])),lit.(:init),:cerl.c_cons_skel(x,lit.([]))],[lit.(1),lit.(2),lit.(3)],[lit.(:a),lit.(:b),lit.(:c)]]
 end
 Enum.map(args,fn args->{:cerl.c_call(lit.(m),lit.(f),args),mfa}end)
end)
base=base++Enum.map(base,fn {body,mfa}->{:cerl.c_fun([],body),mfa}end)
profile=%{module: "dialyzer_typesig",original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path)))),fun_table: fun_table}
rows=base|>Enum.with_index()|>Enum.map(fn {{body,mfa},i}->
 name=String.to_atom("synthetic#{i}")
 variable=v.({:entry,3});fun=:cerl.c_fun([x,y,z],body)
 module=:cerl.c_module(lit.(name),[variable],[],[{variable,fun}])
 exported=CoreExport.export_module(name,[],module)
 {module,next}=:cerl_trees.label(module,0)
 [{variable,fun}|_]=:cerl.module_defs(module)
 plt=shared_plt
 cg=:dialyzer_callgraph.new()
 :dialyzer_callgraph.scan_core_tree(module,cg)
 {m,f,a}=mfa
 signatures=case :dialyzer_plt.lookup(plt,mfa) do
  :none->[]
  {:value,{ret,args}}->[%{mfa: %{module: Atom.to_string(m),function: Atom.to_string(f),arity: a},arguments: Enum.map(args,&NativeTypeWire.encode/1),return: NativeTypeWire.encode(ret)}]
 end
 prior_contracts=case :dialyzer_plt.lookup_contract(plt,mfa) do
  :none->[]
  {:value,{:contract,alternatives,arguments,_}=contract}->[%{mfa: %{module: Atom.to_string(m),function: Atom.to_string(f),arity: a},arguments: Enum.map(arguments,&NativeTypeWire.encode/1),alternatives: Enum.map(alternatives,fn {signature,constraints}->%{signature: NativeTypeWire.encode(signature),constraints: Enum.map(constraints,fn {:subtype,l,r}->[NativeTypeWire.encode(l),NativeTypeWire.encode(r)]end)}end),captured_record: CoreExport.export_terms([contract])}]
 end
 state={:state,cg,:undefined,[],%{},%{},%{},false,false,name,%{},next,false,plt,%{},[{name,%{}}],[],[{name,:entry,3}],[:v2]}
 state=:dialyzer_typesig.state__store_funs([variable],[fun],state)
 {{state,_},allocations}=trace_allocations.(fn -> :dialyzer_typesig.traverse(fun,:sets.from_list([:cerl_trees.get_label(variable)]),state) end)
 functions=elem(state,4)|>Enum.sort()|>Enum.map(fn {f,body}->{flat,_,root_id}=encode.(encode,body,{[],0});%{function: wire_id.(:erl_types.t_var_name(f)),graph: %{root: root_id,nodes: Enum.reverse(flat)}}end)
 next_label=elem(state,11)
 solved=:dialyzer_typesig.solve(:dialyzer_typesig.state__scc(state),:dialyzer_typesig.state__finalize(state))
 :dialyzer_callgraph.delete(cg)
 bindings=solved|>Enum.sort()|>Enum.map(fn {label,type}->%{id: wire_id.(label),type: NativeTypeWire.encode(relative.(relative,type))}end)
 %{indexed_callgraph: true,entry_names: ["entry"],prior_signatures: signatures,prior_contracts: prior_contracts,base_plt_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(base_plt))),program: %{version: 5,profile: CoreExport.profile(),modules: [exported]},profile: profile,native: %{status: "generated",functions: functions,next_label: next_label,bindings: bindings,refold_allocations: allocations}}
end)
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows),original_beam_profile: profile.original_sha256})

:dialyzer_plt.delete(shared_plt)
