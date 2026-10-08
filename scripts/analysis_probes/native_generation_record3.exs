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
      %{derived: true,closure: %{kind: "local",module: Atom.to_string(info.module),name: Atom.to_string(info.name),new_index: info.new_index,uniq: info.uniq,arity: info.arity,env_count: length(info.env),captures: SourceCaptureWire.encode(relative.(relative,info.env),reference)}}
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
leaf=[:a,:b,0,1,1.0,-0.0,1_000_000_000_000_000_000_000_000_000,[],{},[1,2],%{},<<3::3>>]
base=Enum.flat_map(leaf,fn a->[lit.(a),:cerl.c_tuple_skel([lit.(a),x]),:cerl.c_cons_skel(lit.(a),y),:cerl.c_cons_skel(x,lit.([a])),:cerl.c_let([z],lit.(a),:cerl.c_tuple_skel([z,x])),:cerl.c_seq(lit.(a),x)]end)
base=base++[x,y,z,:cerl.c_values([x,x]),:cerl.c_values([x,y]),:cerl.c_values([]),:cerl.c_alias(z,:cerl.c_tuple_skel([x,y])),fail,:cerl.c_catch(fail),:cerl.c_fun([y],:cerl.c_tuple_skel([x,y])),:cerl.c_letrec([{v.({:local,1}),:cerl.c_fun([y],:cerl.c_cons_skel(x,y))}],v.({:local,1})),:cerl.c_let([x,y],:cerl.c_values([z,z]),:cerl.c_tuple_skel([x,y]))]
bifs=[{:+,2},{:-,2},{:*,2},{:<,2},{:"=<",2},{:>,2},{:>=,2},{:and,2},{:or,2},{:not,1},{:"=:=",2},{:==,2},{:is_atom,1},{:is_integer,1},{:is_list,1},{:is_tuple,1},{:is_number,1},{:is_binary,1},{:is_function,1},{:is_function,2},{:abs,1},{:hd,1},{:tl,1},{:length,1},{:tuple_size,1},{:get_module_info,1},{:++,2}]
calls=Enum.flat_map(bifs,fn {f,a}->
  args=if a==1,do: [[x]]++Enum.map(leaf,&[lit.(&1)]),else: [[x,y]]++Enum.flat_map(leaf,fn l->[[x,lit.(l)],[lit.(l),y],[lit.(l),lit.(l)]]end)
  Enum.map(args,&:cerl.c_call(lit.(:erlang),lit.(f),&1))
end)
base=base++calls++[:cerl.c_apply(:cerl.c_fun([y],:cerl.c_tuple_skel([x,y])),[z]),:cerl.c_apply(x,[y]),:cerl.c_let([x],:cerl.c_fun([y],y),:cerl.c_apply(x,[z])),:cerl.c_call(x,y,[:cerl.c_primop(lit.(:raise),[])]),:cerl.c_call(lit.(:missing_library),lit.(:missing_function),[x])]
sizes=[x,lit.(0),lit.(1),lit.(8),lit.(64),lit.(:all),lit.(:undefined),lit.(:bad)]
segments=for size<-sizes,unit<-[1,8],typ<-[:integer,:float,:binary,:utf8,:utf16,:utf32],value<-[x,lit.(7),lit.(1.0),lit.(<<1,2>>)],do: :cerl.c_bitstr(value,size,lit.(unit),lit.(typ),lit.([:unsigned,:big]))
base=base++segments++[:cerl.c_binary([])]++Enum.map(segments,&:cerl.c_binary([&1]))
try_args=[x,lit.(:a),fail,:cerl.c_values([x,y]),:cerl.c_fun([y],y)]
try_bodies=[z,lit.(:ok),fail,:cerl.c_tuple_skel([z,x]),:cerl.c_fun([],z)]
try_handlers=[v.(:reason),lit.(:caught),fail,:cerl.c_tuple_skel([v.(:kind),v.(:reason)]),:cerl.c_fun([],v.(:reason))]
tries=for arg<-try_args,body<-try_bodies,handler<-try_handlers,do: :cerl.c_try(arg,[z],body,[v.(:kind),v.(:reason),v.(:trace)],handler)
base=base++tries++Enum.map(tries,fn inner->:cerl.c_try(fail,[z],z,[v.(:kind),v.(:reason),v.(:trace)],inner)end)
base=base++Enum.map([:build_stacktrace,:recv_peek_message],fn primop->:cerl.c_primop(lit.(primop),[])end)++Enum.map([x,lit.(:infinity),lit.(0),lit.(42),lit.(-1),lit.(:bad),fail],&:cerl.c_primop(lit.(:recv_wait_timeout),[&1]))
pats=[z,lit.(:a),:cerl.c_tuple_skel([z,y]),:cerl.c_cons_skel(z,y),:cerl.c_alias(z,:cerl.c_tuple_skel([x,y]))]
guards=[lit.(:true),lit.(:false),:cerl.c_call(lit.(:erlang),lit.(:is_integer),[z]),:cerl.c_call(lit.(:erlang),lit.(:is_atom),[z]),:cerl.c_try(lit.(:a),[z],z,[v.(:kind),v.(:reason),v.(:trace)],lit.(:false))]
bodies=[z,lit.(:ok),fail,:cerl.c_tuple_skel([z,x]),:cerl.c_fun([],z)]
cases=for pat<-pats,guard<-guards,body<-bodies,do: :cerl.c_case(x,[:cerl.c_clause([pat],guard,body),:cerl.c_clause([z],lit.(:true),lit.(:fallback))])
cases=cases++Enum.map([0,1,2,15,16],fn n->:cerl.c_case(x,List.duplicate(:cerl.c_clause([z],lit.(:true),fail),n))end)
map_keys=[lit.(:a),lit.(:b),x,lit.(1),lit.(%{})]
map_values=[x,y,lit.(:ok),fail]
map_entries=for key<-map_keys,value<-map_values,op<-[:assoc,:exact],do: if(op==:exact,do: :cerl.c_map_pair_exact(key,value),else: :cerl.c_map_pair(key,value))
map_bodies=for arg<-[x,lit.(%{}),fail],entries<-[[]]++Enum.map(map_entries,&[&1])++[[:cerl.c_map_pair(lit.(:a),x),:cerl.c_map_pair_exact(lit.(:a),y)],[:cerl.c_map_pair_exact(lit.(:a),x),:cerl.c_map_pair(lit.(:a),y)],[:cerl.c_map_pair(x,y),:cerl.c_map_pair_exact(lit.(:b),z)]],do: :cerl.c_map(arg,entries)
literal_maps=Enum.map([%{},%{a: 1},%{a: %{b: 2}},{%{a: 1},%{}},[%{a: 1}|:tail],{%{a: %{b: 2}},[%{}]}],lit)
map_patterns=literal_maps++(for key<-[lit.(:a),lit.(1),x],value<-[z,lit.(:ok),:cerl.c_tuple_skel([z,y])],do: :cerl.c_map_pattern([:cerl.c_map_pair_exact(key,value)]))
map_cases=for pat<-map_patterns,guard<-[lit.(:true),lit.(:false)],body<-[z,lit.(:ok),fail],do: :cerl.c_case(x,[:cerl.c_clause([pat],guard,body),:cerl.c_clause([z],lit.(:true),lit.(:fallback))])
binary_patterns=for size<-[lit.(0),lit.(1),lit.(8),y],unit<-[1,8],typ<-[:integer,:float,:binary,:utf8],flags<-[[:unsigned,:big],[:signed,:little]],do: :cerl.c_binary([:cerl.c_bitstr(z,size,lit.(unit),lit.(typ),lit.(flags))])
binary_cases=for pat<-binary_patterns,guard<-[lit.(:true),lit.(:false)],do: :cerl.c_case(x,[:cerl.c_clause([pat],guard,z),:cerl.c_clause([z],lit.(:true),lit.(:fallback))])
base=base++cases++map_bodies++map_cases++binary_cases
base=base++Enum.map(base,fn body->:cerl.c_fun([],body)end)
profile=%{module: "dialyzer_typesig",original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path)))),fun_table: fun_table}
record_cases=for field_type<-[:erl_types.t_integer(),:erl_types.t_atom(),:erl_types.t_any(),:erl_types.t_none()],tag<-[lit.(:entry_record),lit.(:missing_record),y],arity<-[lit.(1),lit.(2),lit.(3),z],nested<-[false,true],cached<-[false,true],guarded<-[false,true] do
 fields=[{:field,{:type,0,:term,[]},field_type}]
 table=%{{:record,:entry_record}=>{{~c"source_fixture.erl",1},[{1,fields}]}}
 test=:cerl.c_call(lit.(:erlang),lit.(:is_record),[x,tag,arity])
 body=if guarded,do: :cerl.c_case(test,[:cerl.c_clause([lit.(true)],lit.(true),x),:cerl.c_clause([lit.(false)],lit.(true),fail)]),else: test
 body=if nested,do: :cerl.c_fun([],body),else: body
 {body,table,cached}
end
base=record_cases
rows=base|>Enum.with_index()|>Enum.map(fn {{body,record_table,cached},i}->
 name=String.to_atom("synthetic#{i}")
 variable=v.({:entry,3});fun=:cerl.c_fun([x,y,z],body)
 module=:cerl.c_module(lit.(name),[variable],[],[{variable,fun}])
 exported=CoreExport.export_module(name,[],module)
 {module,next}=:cerl_trees.label(module,0)
 [{variable,fun}]=:cerl.module_defs(module)
 plt=:dialyzer_plt.new()
 cg=:dialyzer_callgraph.new()
 cserver=:dialyzer_codeserver.new()
 cserver=:dialyzer_codeserver.store_temp_records(name,record_table,cserver)
 cserver=:dialyzer_codeserver.finalize_records(cserver)
 initial_cache=if cached,do: [{name,record_table}],else: []
 state={:state,cg,cserver,[],%{},%{},%{},false,false,name,%{{name,:entry,3}=>variable},next,false,plt,%{},initial_cache,[:erl_types.t_var(:cerl_trees.get_label(fun))],[{name,:entry,3}],[:v2]}
 state=:dialyzer_typesig.state__store_funs([variable],[fun],state)
 {{state,_},allocations}=trace_allocations.(fn -> :dialyzer_typesig.traverse(fun,:sets.from_list([:cerl_trees.get_label(variable)]),state) end)
 Process.put(:header_scope,"source_phase");Process.put(:header_identities,%{})
 functions=elem(state,4)|>Enum.sort()|>Enum.map(fn {f,body}->{flat,_,root_id}=encode.(encode,body,{[],0});%{function: wire_id.(:erl_types.t_var_name(f)),graph: %{root: root_id,nodes: Enum.reverse(flat)}}end)
 next_label=elem(state,11)
 solved=:dialyzer_typesig.solve(:dialyzer_typesig.state__scc(state),:dialyzer_typesig.state__finalize(state))
 source_headers=headers.(cg,cserver,plt)
 source_capture=%{headers: source_headers,record_tables: [%{module: 0,source_terms: CoreExport.export_terms(record_table)}],initial_record_cache: (if cached,do: [0],else: []),solvers: ["v2"]}
 native_source_state=SourceCaptureWire.encode([state],reference)
 :dialyzer_plt.delete(plt)
 :dialyzer_callgraph.delete(cg)
 :dialyzer_codeserver.delete(cserver)
 bindings=solved|>Enum.sort()|>Enum.map(fn {label,type}->%{id: wire_id.(label),type: NativeTypeWire.encode(relative.(relative,type))}end)
 record_declarations=Enum.flat_map(record_table,fn {{:record,tag},{_,arities}}->Enum.map(arities,fn {_arity,fields}->%{module: 0,tag: Atom.to_string(tag),fields: Enum.map(fields,&NativeTypeWire.encode(elem(&1,2)))}end)end)
 %{record_declarations: record_declarations,source_capture: source_capture,native_source_state: native_source_state,program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},profile: profile,native: %{status: "generated",functions: functions,next_label: next_label,bindings: bindings,refold_allocations: allocations}}
end)
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows),original_beam_profile: profile.original_sha256})
