[root,commands,out]=System.argv()
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
src=Path.join(root,".cache/upstream/otp/lib/dialyzer/src/dialyzer_typesig.erl")
copy=Path.join(Path.dirname(out),"native_typesig_tree_probe.erl")
helper="""
probe_bif_rhs(MFA,Lhs) ->
  {_,_,Arity}=MFA,
  Args=[erl_types:t_var(I) || I<-lists:seq(1,Arity)],
  Cs=get_bif_constr(MFA,erl_types:t_var(100),Args,#state{}),
  probe_find_rhs(Cs,erl_types:t_var(Lhs)).
probe_find_rhs(#constraint{lhs=Lhs,rhs=Rhs},Lhs) -> Rhs;
probe_find_rhs(#constraint{},_) -> not_found;
probe_find_rhs(#constraint_list{list=Cs},Lhs) -> probe_find_rhs_list(Cs,Lhs).
probe_find_rhs_list([C|Cs],Lhs) ->
  case probe_find_rhs(C,Lhs) of not_found -> probe_find_rhs_list(Cs,Lhs); Rhs -> Rhs end;
probe_find_rhs_list([],_) -> not_found.
probe_state(Funs) ->
  Self = [Id || {Id,_,_,_,_,true} <- Funs],
  #state{cmap=maps:from_list([{Id,Cs} || {Id,Cs,_,_,_,_} <- Funs]),
         fun_map=maps:from_list([{Id,Rec} || {Id,_,_,Rec,_,_} <- Funs, Rec =/= undefined]),
         fun_arities=maps:from_list([{Id,Arity} || {Id,_,Arity,_,_,_} <- Funs]),
         prop_types=maps:from_list([{erl_types:t_var_name(Id),Prop} || {Id,_,_,_,Prop,_} <- Funs, Prop =/= undefined]),
         self_rec=case Self of [] -> false; [Id] -> Id end}.
"""
File.write!(copy,(File.read!(src)|>String.replace("-module(dialyzer_typesig).","-module(native_typesig_tree_probe).\n-compile(export_all)."))<>"\n"<>helper)
{:ok,:native_typesig_tree_probe,beam}=:compile.file(String.to_charlist(copy),[:binary,{:i,String.to_charlist(Path.dirname(src))},:nowarn_export_all])
{:module,:native_typesig_tree_probe}=:code.load_binary(:native_typesig_tree_probe,String.to_charlist(copy),beam)
key=fn id->cond do
 is_map(id["raw_variable_record"])->:erl_types.t_var(String.to_integer(id["raw_variable_record"]["integer"]))
 id["atom"]->String.to_atom(id["atom"])
 true->String.to_integer(id["integer"])
 end end
id=fn x->cond do
 is_atom(x)->%{atom: Atom.to_string(x)}
 is_integer(x)->%{integer: Integer.to_string(x)}
 true->%{raw_variable_record: %{integer: Integer.to_string(:erl_types.t_var_name(x))}}
 end end
results=commands|>File.read!()|>String.split("\n",trim: true)|>Enum.map(fn line->
 c=:json.decode(line);map=Map.new(c["bindings"],fn e->{key.(e["id"]),TypeCodec.type(e["type"])}end)
 nodes=Enum.reduce(Enum.with_index(c["tree"]),%{},fn {node,index},nodes->
  n=case node["kind"] do
    "reference" -> {:constraint_ref,:erl_types.t_var(key.(node["function"])),Enum.map(node["dependencies"],key)}
    "atomic"->lhs=TypeCodec.type(node["left"]);rhs=if is_integer(node["right"]["derived"]) do
      d=Enum.at(c["derived_nodes"],node["right"]["derived"]);m=d["native_mfa"]
      :native_typesig_tree_probe.probe_bif_rhs({String.to_atom(m["module"]),String.to_atom(m["function"]),m["arity"]},d["native_lhs"])
    else
      TypeCodec.type(node["right"])
    end;deps=:native_typesig_tree_probe.find_constraint_deps([lhs,rhs]);{:constraint,lhs,String.to_atom(node["relation"]),rhs,deps}
    kind->children=Enum.map(node["children"],&Map.fetch!(nodes,&1));deps=:native_typesig_tree_probe.calculate_deps(children);masks=:native_typesig_tree_probe.calculate_masks(children,1,[]);{:constraint_list,String.to_atom(kind),children,deps,masks,{:list,index}}
  end
  Map.put(nodes,index,n)
 end)
 functions=Enum.map(c["functions"]||[],fn f ->
  rec=if is_map(f["recursive_variable"]),do: :erl_types.t_var(key.(f["recursive_variable"])),else: :undefined
  prop=if is_list(f["propagated_domain"]),do: :erl_types.t_fun(Enum.map(f["propagated_domain"],&TypeCodec.type/1),:erl_types.t_any()),else: :undefined
  {:erl_types.t_var(key.(f["id"])),Map.fetch!(nodes,f["body"]),f["arity"],rec,prop,f["self_recursive"]||false}
 end)
 state=:native_typesig_tree_probe.probe_state(functions)
 try do
  result=if is_list(c["scc"]) do
    scc=Enum.map(c["scc"],fn id ->:erl_types.t_var(key.(id))end)
    users=:native_typesig_tree_probe.comp_users(scc,state)
    map=:native_typesig_tree_probe.solve_scc(scc,map,state,users,scc,false)
    {:ok,map,:undefined,[]}
  else
    :native_typesig_tree_probe.v2_solve(Map.fetch!(nodes,c["root"]),map,{:v2_state,%{},state})
  end
  case result do
   {:error,_}->%{status: "failed"}
   {:ok,map,_state,updated}->%{status: "solved",bindings: Enum.map(Enum.sort(map),fn {k,v}->%{id: id.(k),type: NativeTypeWire.encode(v)}end),updated: Enum.map(updated,id)}
  end
 catch kind,reason->
   {class,payload}=case reason do
    {:badmatch,v} when is_atom(v)->{:badmatch,%{term: "atom",value: Atom.to_string(v)}}
    {name,v} when name in [:case_clause,:badmatch,:bad_generator]->{name,if(is_boolean(v),do: v,else: NativeTypeWire.encode(v))}
    {:bad_generators,{left,right}}->{:bad_generators,[Enum.map(left,&NativeTypeWire.encode/1),Enum.map(right,&NativeTypeWire.encode/1)]}
    name when is_atom(name)->{name,:null}
   end
   %{status: "native_error",class: Atom.to_string(class),payload: payload,detail: inspect({kind,reason})}
  end
end)
File.write!(out,:json.encode(results))
