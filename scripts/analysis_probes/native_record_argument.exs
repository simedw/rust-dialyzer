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
d=:erl_types.t_var(1);var=:erl_types.t_var(2);tag=:erl_types.t_var(3);arity=:erl_types.t_var(4)
destinations=[:erl_types.t_atom(true),:erl_types.t_atom(false),:erl_types.t_boolean(),:erl_types.t_any(),:erl_types.t_none(),:erl_types.t_unit()]
tags=[:erl_types.t_atom(:entry_record),:erl_types.t_atom(:missing_record),:erl_types.t_atoms([:entry_record,:missing_record]),:erl_types.t_atom(),:erl_types.t_integer(),:erl_types.t_tuple(),:erl_types.t_none(),:erl_types.t_unit()]
arities=Enum.map([-1,0,1,2,3,1024,1025],&:erl_types.t_integer/1)++[:erl_types.t_integers([1,2]),:erl_types.t_from_range(1,1024),:erl_types.t_integer(),:erl_types.t_float(),:erl_types.t_none(),:erl_types.t_unit(),:erl_types.t_any()]
find=fn recur,term->
 case term do
  {:constraint,_,_,{:fun_var,function,_,_},_}->if Keyword.get(:erlang.fun_info(function),:name)==:"-get_bif_constr/4-fun-13-",do: function,else: nil
  {:constraint_list,_,list,_,_,_}->Enum.find_value(list,&recur.(recur,&1))
  _->nil
 end
end
rows=for field_type<-[:erl_types.t_integer(),:erl_types.t_atom(),:erl_types.t_any(),:erl_types.t_none()],cached<-[false,true],destination<-destinations,tag_type<-tags,arity_type<-arities do
 fields=[{:field,{:type,0,:term,[]},field_type}]
 table=%{{:record,:entry_record}=>{{~c"source_fixture.erl",1},[{1,fields}]}}
 cserver=:dialyzer_codeserver.new();cserver=:dialyzer_codeserver.store_temp_records(:record_direct,table,cserver);cserver=:dialyzer_codeserver.finalize_records(cserver)
 state={:state,:undefined,cserver,[],%{},%{},%{},false,false,:record_direct,%{},100,false,:undefined,%{},(if cached,do: [{:record_direct,table}],else: []),[],[],[:v2]}
 constraints=:dialyzer_typesig.get_bif_constr({:erlang,:is_record,3},d,[var,tag,arity],state)
 function=find.(find,constraints)
 true=is_function(function)
 native=try do %{ok: NativeTypeWire.encode(function.(%{1=>destination,3=>tag_type,4=>arity_type}))}catch class,reason->%{error: %{class: Atom.to_string(class),reason: if(is_atom(reason),do: Atom.to_string(reason),else: inspect(reason))}}end
 :dialyzer_codeserver.delete(cserver)
 %{destination: NativeTypeWire.encode(destination),tag: NativeTypeWire.encode(tag_type),arity: NativeTypeWire.encode(arity_type),field: NativeTypeWire.encode(field_type),cached: cached,native: native,profile: %{original_sha256: Base.encode16(:crypto.hash(:sha256,File.read!(List.to_string(path))))}}
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows)})
