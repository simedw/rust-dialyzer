Code.require_file("native_types.exs",__DIR__)
Code.require_file("core_export.exs",__DIR__)
Code.ensure_loaded!(:erl_types)
Code.ensure_loaded!(:erl_bif_types)
defmodule CallOracle do
 def var(name),do: {:var,0,name}
 def atom(name),do: {:atom,0,name}
 def tuple(items),do: {:tuple,0,items}
 def call(name,args),do: {:call,0,atom(name),args}
 def match(name,value),do: {:match,0,var(name),value}
 def names({:var,_,name}),do: [name]
 def names(tuple) when is_tuple(tuple),do: tuple|>Tuple.to_list()|>Enum.flat_map(&names/1)
 def names(list) when is_list(list),do: Enum.flat_map(list,&names/1)
 def names(_),do: []
 def matches(term,target) do
  here=case term do {:match,_,pattern,_}->if target in names(pattern),do: [term],else: [];_->[] end
  children=cond do is_tuple(term)->Tuple.to_list(term);is_list(term)->term;true->[]end
  here++Enum.flat_map(children,&matches(&1,target))
 end
 def binding(body,name) do
  case matches(body,name) do [one]->one;other->raise "pinned call source expected one #{name} binding, got #{length(other)}" end
 end
 def expr(text) do
  {:ok,tokens,_}=:erl_scan.string(String.to_charlist(text<>"."));{:ok,[expression]}=:erl_parse.parse_exprs(tokens);expression
 end
 def rewrite_opacity({:call,_line,{:atom,_,:format_type},[value,_state]}),do: value
 def rewrite_opacity({:call,line,{:atom,a,:opacity_conflicts_1},args}),do: {:call,line,{:atom,a,:call_opacity_1},Enum.map(args,&rewrite_opacity/1)}
 def rewrite_opacity(term) when is_tuple(term),do: term|>Tuple.to_list()|>Enum.map(&rewrite_opacity/1)|>List.to_tuple()
 def rewrite_opacity(term) when is_list(term),do: Enum.map(term,&rewrite_opacity/1)
 def rewrite_opacity(term),do: term
 def pure(forms) do
  [{:function,_,:handle_apply_or_call,10,clauses}]=Enum.filter(forms,&match?({:function,_,:handle_apply_or_call,10,_},&1))
  [clause]=Enum.filter(clauses,fn {:clause,_,_,_,body}->Enum.any?(body,&match?({:match,_,{:var,_,:Any},_},&1))end)
  {:clause,_,_,_,body}=clause
  names=[:Any,:AnyArgs,:GenSig,:CArgs,:BifArgs,:SigArgs,:NewArgsSig,:NewArgsContract,:NewArgsBif,:NewArgTypes0,:NewArgTypes,:BifRet,:ContrRet,:RetWithoutContr,:RetWithoutLocal,:FailedConj,:IsFailBif,:IsFailSig]
  setup=[match(:Args,var(:ArgTypes))]++Enum.map(names,&binding(body,&1))
  {:match,_,_,{:case,_,outer,outer_clauses}}=binding(body,:State2)
  [{:clause,_,[{:atom,_,true}],[],[{:case,_,inner,_}]}]=Enum.filter(outer_clauses,&match?({:clause,_,[{:atom,_,true}],[],_},&1))
  failures=Enum.map([:FailedSig,:FailedContract,:FailedBif,:InfSig,:FailReason],&binding(body,&1))
  diagnostics={:case,0,outer,[
   {:clause,0,[atom(false)],[],[atom(:none)]},
   {:clause,0,[atom(true)],[],[{:case,0,inner,[
    {:clause,0,[atom(true)],[],[tuple([atom(:contract_range),var(:RetWithoutContr)])]},
    {:clause,0,[atom(false)],[],failures++[expr("{failing_call,FailedSig,FailedContract,FailedBif,FailReason,[I || {I,T} <- lists:enumerate(NewArgTypes),t_is_none(T)],InfSig}")]}
   ]}]}
  ]}
  final=[match(:Diagnostic,diagnostics),binding(body,:TotalRet),match(:State,expr("#state{module=Module}")),
   match(:ExpectedOpacity,expr("t_inf_lists(CArgs,SigArgs)")),match(:Opacity,expr("call_opacity_1(ArgTypes,ExpectedOpacity,State,1,none,[])")),
   expr("{SigArgs,CArgs,BifArgs,NewArgsSig,NewArgsContract,NewArgsBif,NewArgTypes,SigRange,ContrRet,BifRet,RetWithoutContr,RetWithoutLocal,LocalRet,TotalRet,FailedConj,IsFailSig,IsFailBif,Diagnostic,Opacity,ExpectedOpacity}")]
  entry={:function,0,:call_transfer,7,[{:clause,0,Enum.map([:TypeOfApply,:Fun,:Sig,:Contr,:LocalRet,:ArgTypes,:Module],&var/1),[],setup++final}]}
  [{:function,line,:opacity_conflicts_1,arity,opacity_clauses}]=Enum.filter(forms,&match?({:function,_,:opacity_conflicts_1,_,_},&1))
  opacity={:function,line,:call_opacity_1,arity,rewrite_opacity(opacity_clauses)}
  accumulator={:function,0,:call_accumulate,5,[{:clause,0,Enum.map([:AccArgTypes,:AccRet,:FailedConj,:NewArgTypes,:TotalRet],&var/1),[],[binding(body,:NewAccArgTypes),binding(body,:NewAccRet),expr("{NewAccArgTypes,NewAccRet}")]}]}
  consensus={:function,0,:call_consensus,3,[{:clause,0,Enum.map([:HowMany,:OldWarnings,:NewWarnings],&var/1),[],[binding(body,:NewWarns),var(:NewWarns)]}]}
  [external_clause]=Enum.filter(clauses,fn {:clause,_,_,_,body}->Enum.any?(body,&match?({:match,_,{:var,_,:NewHowMany},_},&1))end)
  {:clause,_,_,_,external_body}=external_clause
  external={:function,0,:call_external_consensus,1,[{:clause,0,[var(:HowMany)],[],[binding(external_body,:NewHowMany),binding(external_body,:NewWarns),var(:NewWarns)]}]}
  [entry,opacity,accumulator,consensus,external]
 end
 def load(module,alias,extra) do
  path=:code.which(module)
  {:ok,{^module,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(path,[:abstract_code])
  additions=extra.(forms)
  transformed=Enum.map(forms,fn {:attribute,line,:module,^module}->{:attribute,line,:module,alias};other->other end)++additions
  {:ok,^alias,binary,_}=:compile.forms(transformed,[:binary,:export_all,:return_errors,:return_warnings])
  {:module,^alias}=:code.load_binary(alias,path,binary)
  %{module: module,beam_sha256: :crypto.hash(:sha256,File.read!(List.to_string(path)))|>Base.encode16(case: :lower),factor_sha256: :crypto.hash(:sha256,:erlang.term_to_binary(additions))|>Base.encode16(case: :lower)}
 end
 def decode(%{"kind"=>_}=term),do: CoreExport.decode(term)
 def decode(term),do: NativeTypes.decode(term)
 def contract(nil),do: :none
 def contract(data) do
  pairs=Enum.map(data["alternatives"],fn alt->{decode(alt["signature"]),Enum.map(alt["constraints"]||[],fn c->{:subtype,decode(c["left"]),decode(c["right"])}end)}end)
  args=apply(:call_native_contracts,:general_domain,[pairs])
  {:value,{:contract,pairs,args,[]}}
 end
 def input(data) do
  actual=Enum.map(data["actual"],&decode/1)
  sig=case data["success"] do nil->:none;s->{:value,{decode(s["result"]),Enum.map(s["arguments"],&decode/1)}}end
  contr=contract(data["contract"])
  local=decode(data["local_return"]||%{"constructor"=>"t_any","arguments"=>[]})
  [m,f,a]=data["mfa"]||["unknown","function",length(actual)]
  {String.to_atom(data["mode"]||"remote"),{String.to_atom(m),String.to_atom(f),a},sig,contr,local,actual,String.to_atom(data["module"]||"caller")}
 end
 def accumulate(data,profile) do
  actual=Enum.map(data["actual"],&decode/1)
  initial={Enum.map(actual,fn _->:erl_types.t_none()end),:erl_types.t_none(),false,:none,[]}
  {result,inputs}=Enum.map_reduce(data["candidates"],initial,fn candidate,{accargs,accret,external,count,oldwarnings}->
   if candidate["external"] do
    {count,warnings}=apply(:call_native_dataflow,:call_external_consensus,[count])
    state={actual,:erl_types.t_any(),true,count,warnings}
    {%{external: true},state}
   else
    request=Map.put(candidate,"actual",data["actual"])
    normalized=input(request)
    value=apply(:call_native_dataflow,:call_transfer,Tuple.to_list(normalized))
    {args,ret}=apply(:call_native_dataflow,:call_accumulate,[accargs,accret,elem(value,14),elem(value,6),elem(value,13)])
    warnings=Enum.map(candidate["warnings"]||[],&String.to_atom/1)
    {count,warnings}=apply(:call_native_dataflow,:call_consensus,[count,oldwarnings,warnings])
    {%{input: CoreExport.export_terms(normalized),outcome: %{status: "ok",value: CoreExport.export_terms(value)},warnings: candidate["warnings"]||[]},{args,ret,external,count,warnings}}
   end
  end)
  %{id: data["id"],operation: "accumulate",actual: CoreExport.export_terms(actual),candidates: result,expected: CoreExport.export_terms(inputs),profile: profile}
 end
 def run(data,profile) do
  args=input(data)
  boundary={elem(args,0),elem(args,1),elem(args,2),elem(args,3),elem(args,4),elem(args,5),elem(args,6)}
  outcome=try do
   value=apply(:call_native_dataflow,:call_transfer,Tuple.to_list(args))
   %{status: "ok",value: CoreExport.export_terms(value)}
  catch kind,reason->%{status: "error",error_kind: kind,reason: CoreExport.export_terms(reason)} end
  %{id: data["id"],input: CoreExport.export_terms(boundary),outcome: outcome,profile: profile}
 end
end
profile=[CallOracle.load(:dialyzer_dataflow,:call_native_dataflow,&CallOracle.pure/1),CallOracle.load(:dialyzer_contracts,:call_native_contracts,fn _->[]end)]
for line<-IO.stream(:stdio,:line) do
 data=:json.decode(line)
 result=if data["operation"]=="accumulate",do: CallOracle.accumulate(data,profile),else: CallOracle.run(data,profile)
 IO.puts(:json.encode(result))
end
