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
refs=fn _recur,value ->
 case value do
  reference when is_reference(reference)->
   scope=Process.get(:header_scope)
   ids=Process.get(:header_identities,%{})
   case Map.fetch(ids,reference) do
    {:ok,id}->%{scope: scope,object: id}
    :error->id="source_reference_#{map_size(ids)}";Process.put(:header_identities,Map.put(ids,reference,id));%{scope: scope,object: id}
   end
  :clean->"clean"
 end
end
reference=fn value->refs.(refs,value)end
digraph=fn {:digraph,vertices,edges,neighbours,cyclic}->%{vertices: reference.(vertices),edges: reference.(edges),neighbours: reference.(neighbours),cyclic: cyclic}end
headers=fn graph,server,plt ->
 cg=if graph==:undefined,do: :null,else: (case graph do
  {:callgraph,g,active,esc,letrec,names,reverse,recursive,selfrec,calls}->%{digraph: digraph.(g),active: case active do :undefined->%{kind: "undefined"};{:d,g}->%{kind: "digraph",value: digraph.(g)};{:e,ref}->%{kind: "outgoing",value: reference.(ref)}end,escaping: reference.(esc),letrec: reference.(letrec),names: reference.(names),reverse_names: reference.(reverse),recursive_variables: reference.(recursive),self_recursive: reference.(selfrec),calls: reference.(calls)}
 end)
 cs=if server==:undefined,do: :null,else: (case server do
  {:codeserver,next,code,exported,records,contracts,callbacks,meta,exports,texported,trecords,tcontracts,tcallbacks}->%{next_core_label: next,code: reference.(code),exported_types: reference.(exported),records: reference.(records),contracts: reference.(contracts),callbacks: reference.(callbacks),function_metadata: reference.(meta),exports: reference.(exports),temporary_exported_types: reference.(texported),temporary_records: reference.(trecords),temporary_contracts: reference.(tcontracts),temporary_callbacks: reference.(tcallbacks)}
 end)
 p=if plt==:undefined,do: :null,else: (case plt do
  {:plt,info,types,contracts,callbacks,exported}->%{signatures: reference.(info),types: reference.(types),contracts: reference.(contracts),callbacks: reference.(callbacks),exported_types: reference.(exported)}
 end)
 %{callgraph: cg,codeserver: cs,plt: p}
end

Code.require_file("source_capture_wire.exs",__DIR__)
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
 result=try do {:ok,function.()} catch class,reason->{:error,class,reason,__STACKTRACE__} end
 ref=:erlang.trace_delivered(self())
 receive do {:trace_delivered,_,^ref}->:ok end
 :erlang.trace(self(),false,[:call])
 reply=make_ref();send(tracer,{:result,reply})
 labels=receive do {^reply,n}->n end
 true=Enum.all?(Enum.chunk_every(labels,2,1,:discard),fn [a,b]->a>b end)
 Process.put(:synthetic_labels,labels|>Enum.with_index(1)|>Map.new(fn {label,index}->{label,-index}end))
 case result do
 {:ok,value}->{value,length(labels)}
 {:error,class,reason,stack}->:erlang.raise(class,reason,stack)
 end
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
x=v.(:x); y=v.(:y)
entry=v.({:entry,1}); helper=v.({:helper,1}); local=v.({:local,1})
call=fn op,arg->:cerl.c_apply(op,[arg])end
fail=:cerl.c_primop(lit.(:match_fail),[])
guard=:cerl.c_call(lit.(:erlang),lit.(:is_integer),[x])
choice=fn recursive->:cerl.c_case(x,[:cerl.c_clause([lit.(0)],lit.(:true),lit.(:done)),:cerl.c_clause([y],lit.(:true),call.(recursive,y))])end
shapes=[
 {lit.(:ok),x}, {x,lit.(0)}, {:cerl.c_tuple_skel([x,lit.(:ok)]),x},
 {:cerl.c_fun([y],:cerl.c_tuple_skel([x,y])),x},
 {:cerl.c_letrec([{local,:cerl.c_fun([y],:cerl.c_tuple_skel([x,y]))}],call.(local,x)),x},
 {call.(entry,x),x}, {choice.(entry),x}, {call.(helper,x),call.(entry,x)},
 {:cerl.c_fun([y],call.(entry,y)),x},
 {:cerl.c_letrec([{local,:cerl.c_fun([y],call.(local,y))}],call.(local,x)),x},
 {:cerl.c_case(x,[:cerl.c_clause([y],guard,y),:cerl.c_clause([y],lit.(:true),fail)]),x},
 {:cerl.c_call(lit.(:erlang),lit.(:+),[x,lit.(1)]),x}
]
profile=%{module: "dialyzer_typesig",original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path)))),fun_table: fun_table}
rows=for {{body,helper_body},index}<-Enum.with_index(shapes),names<-[[],[:entry],[:helper],[:entry,:helper],[:helper,:entry],[:entry,:entry]],offset<-[0,7,10000],solvers<-[[],[:v2],[:v2,:v2],[:v1],[:v1,:v2],[:v2,:v1],[:v1,:v1]] do
 name=String.to_atom("sccfixture#{index}")
 module=:cerl.c_module(lit.(name),[entry,helper],[],[{entry,:cerl.c_fun([x],body)},{helper,:cerl.c_fun([x],helper_body)}])
 exported=CoreExport.export_module(name,[],module)
 {module,next}=:cerl_trees.label(module,0)
 phase_next=next+offset
 plt=:dialyzer_plt.new();cg=:dialyzer_callgraph.new();:dialyzer_callgraph.scan_core_tree(module,cg)
 cserver=:dialyzer_codeserver.new();cserver=:dialyzer_codeserver.insert(name,module,cserver)
 cserver=:dialyzer_codeserver.store_temp_records(name,%{},cserver);cserver=:dialyzer_codeserver.finalize_records(cserver)
 :dialyzer_plt.insert_list(plt,[{{name,:entry,1},{:erl_types.t_any(),[:erl_types.t_any()]}},{{name,:helper,1},{:erl_types.t_any(),[:erl_types.t_any()]}}])
 mfas=Enum.map(names,&{name,&1,1})
 native=try do
  {bindings,allocations}=trace_allocations.(fn ->:dialyzer_typesig.analyze_scc(mfas,phase_next,cg,cserver,plt,[],solvers)end)
  %{status: "analyzed",bindings: Enum.map(bindings,fn {label,type}->%{id: wire_id.(label),type: NativeTypeWire.encode(relative.(relative,type))}end),refold_allocations: allocations}
 catch
  class,reason->%{status: "native_error",class: Atom.to_string(class),reason: if(is_atom(reason),do: Atom.to_string(reason),else: inspect(reason))}
 end
 effective=if solvers==[],do: [:v2],else: solvers
 source_state=if mfas != [] do
  state=:dialyzer_typesig.new_state(mfas,phase_next,cg,cserver,plt,[],effective)
  defset=:sets.from_list(Enum.map(Map.values(elem(state,10)),&:cerl_trees.get_label/1))
  state=:dialyzer_typesig.traverse_scc(mfas,cserver,defset,state)
  Process.put(:header_scope,"source_phase");Process.put(:header_identities,%{})
  source_headers=headers.(cg,cserver,plt)
  capture=%{headers: source_headers,record_tables: [%{module: 0,source_terms: CoreExport.export_terms(%{})}],initial_record_cache: [],solvers: Enum.map(effective,&Atom.to_string/1)}
  %{source_capture: capture,native_source_state: SourceCaptureWire.encode([state],reference)}
 else
  %{}
 end
 :dialyzer_plt.delete(plt);:dialyzer_callgraph.delete(cg);:dialyzer_codeserver.delete(cserver)
 signatures=Enum.map([:entry,:helper],fn function->%{mfa: %{module: Atom.to_string(name),function: Atom.to_string(function),arity: 1},arguments: [NativeTypeWire.encode(:erl_types.t_any())],return: NativeTypeWire.encode(:erl_types.t_any())}end)
 Map.merge(source_state,%{prior_signatures: signatures,analyze_scc: true,entry_mfas: Enum.map(mfas,fn {m,f,a}->%{module: Atom.to_string(m),function: Atom.to_string(f),arity: a}end),phase_next_label: phase_next,solvers: Enum.map(solvers,&Atom.to_string/1),indexed_callgraph: true,entry_names: Enum.map(names,&Atom.to_string/1),program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},profile: profile,native: native})
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows),original_beam_profile: profile.original_sha256})
