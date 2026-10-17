[root,commands,out]=System.argv()
expected_otp=System.get_env("RUST_DIALYZER_CONSTRAINTS_OTP") || "28"; ^expected_otp=List.to_string(:erlang.system_info(:otp_release))
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
src=System.get_env("RUST_DIALYZER_CONSTRAINTS_SOURCE_FILE") || Path.join(root,".cache/upstream/otp/lib/dialyzer/src/dialyzer_typesig.erl")
copy=Path.join(Path.dirname(out),"native_typesig_structural_probe.erl")
helper="""
probe_contract(Pairs,Args,Sig,Map) ->
 Plt=dialyzer_plt:new(), MFA={source_probe,contract,length(Args)},
 C=#contract{contracts=Pairs,args=[t_any() || _ <- Args],forms=[]},
 dialyzer_plt:insert_contract_list(Plt,[{MFA,C}]),
 case Sig of none->ok;{some,Range}->dialyzer_plt:insert_list(Plt,[{MFA,{Range,[t_any() || _ <- Args]}}]) end,
 try
  S=get_plt_constr(MFA,t_var(100),Args,#state{plt=Plt,mfas=[]}),
  probe_find_derived(state__cs(S),t_var(100),Map)
 after dialyzer_plt:delete(Plt) end.
probe_residual(Arg,SubtrTypes,Map) -> t_subtract_list(lookup_type(Arg,Map),SubtrTypes).
probe_maps(map_construct,Arg,Pairs,_Key,_Map,_Shadow) -> lists:foldl(fun({K,V,_},Acc)->erl_types:t_map_put({K,V},Acc)end,erl_types:t_inf(erl_types:t_map(),Arg),Pairs);
probe_maps(map_argument,_Arg,Pairs,_Key,_Map,_Shadow) -> {Res,_}=lists:foldl(fun({K,_,Op},{Acc,Shadow})->New=case Op=:=exact andalso erl_types:t_is_impossible(erl_types:t_inf(Shadow,K)) of true->erl_types:t_map_put({K,erl_types:t_any()},Acc);false->Acc end,{New,erl_types:t_sup(K,Shadow)}end,{erl_types:t_map(),erl_types:t_none()},Pairs),Res;
probe_maps(map_value,_Arg,_Pairs,K,M,Shadow) -> case erl_types:t_is_singleton(K) of false->erl_types:t_any();true->MT=erl_types:t_inf(M,erl_types:t_map()),case erl_types:t_is_impossible(MT)of true->erl_types:t_none();false->case lists:all(fun(SK)->erl_types:t_is_impossible(erl_types:t_inf(SK,K))end,Shadow)of true->erl_types:t_map_get(K,MT);false->erl_types:t_any()end end end.
probe_binary(binary_length,Unit,Mode,Map) -> (bitstr_constr(erl_types:t_var(1),Unit,Mode))(Map);
probe_binary(binary_value,Unit,Signed,Map) -> (bitstr_val_constr(erl_types:t_var(1),Unit,case Signed of true->[signed];false->[] end))(Map).

probe_structural(cons,A,B) -> erl_types:t_cons(A,B);
probe_structural(cons_head,A,_) -> case erl_types:t_is_cons(A) of true->erl_types:t_cons_hd(A);false->erl_types:t_any() end;
probe_structural(cons_tail,A,_) -> case erl_types:t_is_cons(A) of true->erl_types:t_cons_tl(A);false->erl_types:t_any() end;
probe_structural(binary_concat,A,B) -> erl_types:t_bitstr_concat(A,B);
probe_structural(binary_match,A,B) -> erl_types:t_bitstr_match(A,B).

probe_bif_derived(MFA, Lhs, Map) ->
  {_,_,Arity}=MFA,
  Args=[erl_types:t_var(I) || I<-lists:seq(1,Arity)],
  Cs=get_bif_constr(MFA,erl_types:t_var(100),Args,#state{}),
  probe_find_derived(Cs,erl_types:t_var(Lhs),Map).
probe_find_derived(#constraint{lhs=Lhs,rhs=#fun_var{'fun'=Fun}},Lhs,Map) -> Fun(Map);
probe_find_derived(#constraint{},_,_) -> not_found;
probe_find_derived(#constraint_list{list=Cs},Lhs,Map) ->
  probe_find_derived_list(Cs,Lhs,Map).
probe_find_derived_list([C|Cs],Lhs,Map) ->
  case probe_find_derived(C,Lhs,Map) of
    not_found -> probe_find_derived_list(Cs,Lhs,Map);
    Type -> Type
  end;
probe_find_derived_list([],_,_) -> not_found.
"""
File.write!(copy,(File.read!(src)|>String.replace("-module(dialyzer_typesig).","-module(native_typesig_structural_probe).\n-compile(export_all)."))<>"\n"<>helper)
{:ok,:native_typesig_structural_probe,beam}=:compile.file(String.to_charlist(copy),[:binary,{:i,String.to_charlist(Path.dirname(src))},:nowarn_export_all])
{:module,:native_typesig_structural_probe}=:code.load_binary(:native_typesig_structural_probe,String.to_charlist(copy),beam)
results=commands|>File.read!()|>String.split("\n",trim: true)|>Enum.map(fn line->
 c=:json.decode(line)
 map=Map.new(c["bindings"],fn entry->{String.to_integer(entry["id"]["integer"]),TypeCodec.type(entry["type"])}end)
 d=c["derived"];op=String.to_atom(d["operation"]);a=Map.fetch!(map,1);b=Map.fetch!(map,2)
 try do
  type=cond do
   op in [:map_construct,:map_argument,:map_value]->
    lookup=fn term->:erl_types.t_subst(TypeCodec.type(term),map)end
    pairs=Enum.map(d["entries"]||[],fn p->{lookup.(p["key"]),lookup.(p["value"]),String.to_atom(p["operation"])}end)
    :native_typesig_structural_probe.probe_maps(op,if(d["argument"],do: lookup.(d["argument"]),else: :erl_types.t_any()),pairs,if(d["key"],do: lookup.(d["key"]),else: :erl_types.t_any()),if(d["map"],do: lookup.(d["map"]),else: :erl_types.t_any()),Enum.map(d["shadow_keys"]||[],lookup))
   op==:contract_return->
    pairs=Enum.map(d["contract"]["alternatives"],fn alt->{TypeCodec.type(alt["signature"]),Enum.map(alt["constraints"],fn [l,r]->{:subtype,TypeCodec.type(l),TypeCodec.type(r)}end)}end)
    sig=if d["signature_return"]in[nil,:null],do: :none,else: {:some,TypeCodec.type(d["signature_return"])}
    :native_typesig_structural_probe.probe_contract(pairs,Enum.map(d["arguments"],&TypeCodec.type/1),sig,map)
   op==:function_arity_argument->:native_typesig_structural_probe.probe_bif_derived({:erlang,:is_function,2},1,Map.put(map,100,a))
   op==:clause_residual->:native_typesig_structural_probe.probe_residual(TypeCodec.type(d["argument"]),Enum.map(d["subtract"],&TypeCodec.type/1),map)
   op in [:append_head,:append_tail]->:native_typesig_structural_probe.probe_bif_derived({:erlang,:++,2},if(op==:append_head,do: 1,else: 2),Map.put(map,100,a))
   op==:binary_segments->:erl_types.t_bitstr_concat(Enum.map(d["segments"],fn v->:erl_types.t_subst(TypeCodec.type(v),map)end))
   op in [:binary_length,:binary_value] ->
    arg=if op==:binary_length,do: String.to_atom(d["mode"]),else: d["signed"]
    :native_typesig_structural_probe.probe_binary(op,d["unit"],arg,map)
   true ->
    :native_typesig_structural_probe.probe_structural(op,a,b)
   end
  if type==:not_found,do: %{status: "not_found"},else: %{status: "type",type: NativeTypeWire.encode(type)}
 catch kind,reason->
  {class,payload}=case reason do
   name when is_atom(name)->{name,:null}
   {name,v} when name in [:case_clause,:badmatch,:bad_generator]->{name,if(is_boolean(v),do: v,else: NativeTypeWire.encode(v))}
  end
  %{status: "native_error",class: Atom.to_string(class),payload: payload,detail: inspect({kind,reason})}
 end
end)
File.write!(out,:json.encode(results))
