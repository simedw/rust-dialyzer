[root,commands,out]=System.argv()
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
id=fn v->if v["atom"],do: String.to_atom(v["atom"]),else: String.to_integer(v["integer"])end
wire_id=fn v->if is_atom(v),do: %{atom: Atom.to_string(v)},else: %{integer: Integer.to_string(v)}end
wire_ids=fn deps->Enum.map(deps,wire_id)end
encode=fn recur,node,{nodes,count}->
 case node do
  {:constraint,lhs,relation,rhs,deps}-> {[%{kind: "atomic",left: NativeTypeWire.encode(lhs),relation: Atom.to_string(relation),right: NativeTypeWire.encode(rhs),deps: wire_ids.(deps)}|nodes],count+1,count}
  {:constraint_ref,function,deps}->{[%{kind: "ref",function: wire_id.(:erl_types.t_var_name(function)),deps: wire_ids.(deps)}|nodes],count+1,count}
  {:constraint_list,kind,children,deps,_,_}->
   {ids,{nodes,count}}=Enum.map_reduce(children,{nodes,count},fn child,state->{ns,nc,id}=recur.(recur,child,state);{id,{ns,nc}}end)
   {[%{kind: Atom.to_string(kind),children: ids,deps: wire_ids.(deps)}|nodes],count+1,count}
 end
end
encode_prepared=fn recur,node,{nodes,count}->
 case node do
  {:constraint,_,_,_,_}->{[value],_,_}=encode.(encode,node,{[],0});{[value|nodes],count+1,count}
  {:constraint_ref,_,_}->{[value],_,_}=encode.(encode,node,{[],0});{[value|nodes],count+1,count}
  {:constraint_list,kind,children,deps,masks,{:list,list_id}}->
   {ids,{nodes,count}}=Enum.map_reduce(children,{nodes,count},fn child,state->{ns,nc,id}=recur.(recur,child,state);{id,{ns,nc}}end)
   mask_values=masks|>Enum.sort()|>Enum.map(fn {v,indices}->%{variable: wire_id.(v),indices: indices}end)
   {[%{kind: Atom.to_string(kind),children: ids,deps: wire_ids.(deps),list_id: list_id,masks: mask_values}|nodes],count+1,count}
 end
end
results=commands|>File.read!()|>String.split("\n",trim: true)|>Enum.map(fn line->
 c=:json.decode(line)["normalize"]
 try do
  {nodes,_}=Enum.reduce(c["nodes"],{%{},0},fn node,{nodes,index}->
   result=case node["kind"] do
    "atomic"->:dialyzer_typesig.mk_constraint(TypeCodec.type(node["left"]),String.to_atom(node["relation"]),TypeCodec.type(node["right"]))
    "ref"->:dialyzer_typesig.mk_constraint_ref(:erl_types.t_var(id.(node["function"])),Enum.map(node["deps"],id))
    kind->children=Enum.map(node["children"],&Map.fetch!(nodes,&1));if kind=="conj",do: :dialyzer_typesig.mk_conj_constraint_list(children),else: :dialyzer_typesig.mk_disj_constraint_list(children)
   end
   {Map.put(nodes,index,result),index+1}
  end)
  root=Map.fetch!(nodes,c["root"]);root=if c["dnf"]==true,do: :dialyzer_typesig.mk_disj_norm_form(root),else: root
  if c["functions"] do
   pairs=Enum.map(c["functions"],fn f->{:erl_types.t_var(id.(f["function"])),Map.fetch!(nodes,f["body"])}end)
   requested=Enum.map(Map.get(c,"solvers",[]),&String.to_atom/1)
   effective=if requested==[],do: [:v2],else: requested
   state=:dialyzer_typesig.new_state([],0,:undefined,:undefined,:undefined,[],effective)
   state=put_elem(state,4,Map.new(pairs))
   state=put_elem(state,16,Enum.map(pairs,&elem(&1,0)))
   cs=Enum.map(pairs,fn {function,body}->:dialyzer_typesig.mk_constraint_ref(function,:dialyzer_typesig.get_deps(body))end)
   {_,next_list_id,state}=:dialyzer_typesig.enumerate_constraints(cs,0,[],state)
   state=:dialyzer_typesig.order_fun_constraints(state)
   functions=elem(state,4)|>Enum.sort()
   functions=Enum.map(functions,fn {f,body}->{flat,_,root_id}=encode_prepared.(encode_prepared,body,{[],0});%{function: wire_id.(:erl_types.t_var_name(f)),graph: %{root: root_id,nodes: Enum.reverse(flat)}}end)
   %{status: "prepared",graph: %{functions: functions,next_list_id: next_list_id}}
  else
   {flat,_count,root_id}=encode.(encode,root,{[],0})
   %{status: "normalized",graph: %{root: root_id,nodes: Enum.reverse(flat)}}
  end
 catch
  :throw,:error->%{status: "throw_error"}
  kind,reason->
   {class,payload}=case reason do
    name when is_atom(name)->{name,:null}
    {name,v} when name in [:case_clause,:badmatch,:bad_generator]->{name,if(is_boolean(v),do: v,else: NativeTypeWire.encode(v))}
    {:bad_generators,{a,b}}->{:bad_generators,[Enum.map(a,&NativeTypeWire.encode/1),Enum.map(b,&NativeTypeWire.encode/1)]}
   end
   %{status: "native_error",class: Atom.to_string(class),payload: payload,detail: inspect({kind,reason})}
 end
end)
File.write!(out,:json.encode(results))
