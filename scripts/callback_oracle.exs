Code.require_file("type_codec.exs", __DIR__)
defmodule CallbackBoundary do
  def lookup(_, _), do: Process.get(:inferred)
  def is_exported(_, _), do: Process.get(:exported)
  def lookup_mfa_contract(_, _), do: Process.get(:declared)
end
defmodule CallbackOracle do
  def rewrite({:call,a,{:remote,b,{:atom,c,module},{:atom,d,function}},args}) when {module,function} in [{:dialyzer_plt,:lookup},{:dialyzer_codeserver,:is_exported},{:dialyzer_codeserver,:lookup_mfa_contract}] do
    {:call,a,{:remote,b,{:atom,c,CallbackBoundary},{:atom,d,function}},args}
  end
  def rewrite(tuple) when is_tuple(tuple), do: tuple |> Tuple.to_list() |> Enum.map(&rewrite/1) |> List.to_tuple()
  def rewrite(list) when is_list(list), do: Enum.map(list,&rewrite/1)
  def rewrite(value), do: value
  def load do
    {:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_behaviours),[:abstract_code])
    forms=Enum.map(forms, fn {:attribute,a,:module,_}->{:attribute,a,:module,:rust_callback_native}; form->rewrite(form) end) |> Enum.reject(&match?({:eof,_},&1))
    {:ok,tokens,_}=:erl_scan.string(~c"state() -> #state{plt=unused, codeserver=unused,records=maps:new()}.")
    {:ok,helper}=:erl_parse.parse_form(tokens)
    {:ok,:rust_callback_native,binary}=:compile.forms(forms++[helper],[:binary,:export_all])
    {:module,:rust_callback_native}=:code.load_binary(:rust_callback_native,~c"callback",binary)
  end
  def contract(type) do
    signature=TypeCodec.type(type)
    general=:erl_types.subst_all_vars_to_any(signature)
    {:contract,[{signature,[]}],:erl_types.t_fun_args(general),[]}
  end
  def finding({:callback_missing,_}),do: %{kind: "missing"}
  def finding({:callback_not_exported,_}),do: %{kind: "not_exported"}
  def finding({:callback_type_mismatch,[_,_,_,a,e]}),do: %{kind: "return",actual: List.to_string(a),expected: List.to_string(e)}
  def finding({:callback_arg_type_mismatch,[_,_,_,p,a,e]}),do: %{kind: "argument",position: p,actual: List.to_string(a),expected: List.to_string(e)}
  def finding({:callback_spec_type_mismatch,[_,_,_,_,_,a,e]}),do: %{kind: "spec_return",extra: List.to_string(a),expected: List.to_string(e)}
  def finding({:callback_spec_arg_type_mismatch,[_,_,_,_,_,p,a,e]}),do: %{kind: "spec_argument",position: p,actual: List.to_string(a),expected: List.to_string(e)}
end
CallbackOracle.load()
for line<-IO.stream(:stdio,:line) do
  r=:json.decode(line)
  inferred=case r["inferred"] do :null->:none; s->{:value,{TypeCodec.type(s["result"]),Enum.map(s["arguments"],&TypeCodec.type/1)}} end
  declared=case r["declared"] do :null->:error; s->{:ok,{{~c"/spec.erl",5},CallbackOracle.contract(s),[]}} end
  Process.put(:inferred,inferred);Process.put(:declared,declared);Process.put(:exported,r["exported"])
  contract=CallbackOracle.contract(r["callback"])
  arity=length(elem(contract,2))
  callback={{:beh,:f,arity},{{~c"/callback.erl",1},contract,if(r["optional"],do: [:optional_callback],else: [])}}
  warnings=:rust_callback_native.check_all_callbacks(:m,:beh,[callback],:rust_callback_native.state(),[])
  IO.puts(:json.encode(%{id: r["id"],findings: Enum.map(warnings,&CallbackOracle.finding/1)}))
end
