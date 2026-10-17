[root,commands,out]=System.argv()
expected_otp=System.get_env("RUST_DIALYZER_CONSTRAINTS_OTP") || "28"; ^expected_otp=List.to_string(:erlang.system_info(:otp_release))
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
src=System.get_env("RUST_DIALYZER_CONSTRAINTS_SOURCE_FILE") || Path.join(root,".cache/upstream/otp/lib/dialyzer/src/dialyzer_typesig.erl")
copy=Path.join(Path.dirname(out),"native_typesig_derived_probe.erl")
helper="""
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
File.write!(copy,(File.read!(src)|>String.replace("-module(dialyzer_typesig).","-module(native_typesig_derived_probe).\n-compile(export_all)."))<>"\n"<>helper)
{:ok,:native_typesig_derived_probe,beam}=:compile.file(String.to_charlist(copy),[:binary,{:i,String.to_charlist(Path.dirname(src))},:nowarn_export_all])
{:module,:native_typesig_derived_probe}=:code.load_binary(:native_typesig_derived_probe,String.to_charlist(copy),beam)
results=commands|>File.read!()|>String.split("\n",trim: true)|>Enum.map(fn line->
 c=:json.decode(line)
 map=Map.new(c["bindings"],fn entry->{String.to_integer(entry["id"]["integer"]),TypeCodec.type(entry["type"])}end)
 mfa=c["native_mfa"];mfa={String.to_atom(mfa["module"]),String.to_atom(mfa["function"]),mfa["arity"]}
 try do
  type=:native_typesig_derived_probe.probe_bif_derived(mfa,c["native_lhs"],map)
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
