[root,out]=System.argv()
for f<-~w(type_codec.exs type_wire.exs core_export.exs),do: Code.require_file(Path.join(root,"scripts/"<>f))
src=File.read!(Path.join(root,"scripts/dataflow_state_probes/function_state_oracle.exs"))|>String.split("defmodule FunctionStateOracle")|>List.last()|>String.split(" def run(row")|>hd();Code.eval_string("defmodule FunctionStateOracle"<>src<>"end")
{fields,cgfs}=FunctionStateOracle.load()
get=fn s,k->FunctionStateOracle.get(s,fields,k)end
set=fn s,k,v->put_elem(s,Enum.find_index(fields,&(&1==k))+1,v)end
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_dataflow),[:abstract_code]);{:attribute,_,:record,{:map,mapfs}}=Enum.find(forms,&match?({:attribute,_,:record,{:map,_}},&1));mapfs=Enum.map(mapfs,fn f->f=if elem(f,0)==:typed_record_field,do: elem(f,1),else: f;elem(elem(f,2),2)end)
mapget=fn m,k->elem(m,Enum.find_index(mapfs,&(&1==k))+1)end
mapwire=fn m->%{types: Map.new(mapget.(m,:map),fn {k,v}->{Integer.to_string(k),NativeTypeWire.encode(v)}end),substitutions: Map.new(mapget.(m,:subst),fn {k,v}->{Integer.to_string(k),if(is_integer(v),do: v,else: :cerl_trees.get_label(v))}end),modified: mapget.(m,:modified)}end
k=fn s->%{"kind"=>s}end;atom=fn a->%{"kind"=>"atoms","values"=>[to_string(a)]}end
fun=fn as,r->%{"kind"=>"function","domain"=>as,"range"=>r}end
nom=fn t->%{"kind"=>"nominal","name"=>%{"module"=>"other","name"=>"wrapped","arity"=>0,"opacity"=>"opaque"},"inner"=>t}end
pool=Enum.map(~w(any none unit integer atom float nil tuple_any map_any),k)++[atom.(:ok),atom.(true),atom.(false),%{"kind"=>"integers","values"=>[0]},nom.(k.("integer")),nom.(atom.(:ok))]
op_pool=pool++[fun.([k.("integer")],k.("atom")),fun.([k.("atom")],k.("unit")),fun.([k.("atom")],k.("none")),fun.([],k.("integer"))]
sig=fn as,r->%{arguments: as,result: r}end
v=fn a->:cerl.ann_c_var([1],a)end;lit=&:cerl.abstract/1
m=v.(:m);f=v.(:f);x=v.(:x);y=v.(:y);op=v.(:operator);z=v.(:z)
call=fn mod,name,args->:cerl.ann_c_call([1],lit.(mod),lit.(name),args)end
apply=fn args->:cerl.ann_c_apply([1],op,args)end
seq_warning=fn tag,tail,last->:cerl.c_seq(:cerl.ann_c_cons([1],lit.(tag),tail),last)end
shapes=[{"call_order_warnings",:cerl.ann_c_call([1],seq_warning.(:m,x,lit.(:erlang)),seq_warning.(:f,y,lit.(:abs)),[x]),"remote",nil},{"call_none_then_warnings",:cerl.ann_c_call([1],:cerl.c_primop(lit.(:match_fail),[]),seq_warning.(:f,y,lit.(:abs)),[x]),"remote",nil},{"apply_operator_fun",:cerl.ann_c_apply([1],:cerl.c_fun([z],lit.(:operator)),[x]),"fallback",nil},{"unknown",call.(:unknown_mod,:unknown,[x]),"remote",nil},{"zero_unknown",call.(:unknown_mod,:unknown,[]),"remote",nil},{"zero_bif",call.(:erlang,:node,[]),"remote",nil},{"zero_apply",apply.([]),"fallback",nil},{"exit_bif",call.(:erlang,:error,[x]),"remote",nil},{"sig_bif_abs",call.(:erlang,:abs,[x]),"remote","sig_bif_abs"},{"abs",call.(:erlang,:abs,[x]),"remote",nil},{"length",call.(:erlang,:length,[x]),"remote",nil},{"equals",call.(:erlang,:"=:=",[x,y]),"remote",nil},{"inexact",call.(:erlang,:"==",[x,y]),"remote",nil},{"record",call.(:erlang,:is_record,[x,y,lit.(2)]),"remote",nil},{"signature",call.(:library,:typed,[x]),"remote","signature"},{"contract",call.(:library,:typed,[x]),"remote","contract"},{"contract_range",call.(:library,:typed,[x]),"remote","range"},{"contract_overload",call.(:library,:typed,[x]),"remote","overload"},{"bounded_contract",call.(:library,:typed,[x]),"remote","bounded"},{"opacity",call.(:library,:typed,[x]),"remote","opacity"},{"dynamic_module",:cerl.ann_c_call([1],m,lit.(:f),[x]),"dynamic",nil},{"dynamic_function",:cerl.ann_c_call([1],lit.(:erlang),f,[x]),"dynamic",nil},{"apply_unknown",apply.([x]),"fallback",nil},{"apply_external",apply.([x]),"external",nil},{"local_one",apply.([x]),"alpha",nil},{"local_contract",apply.([x]),"alpha","local_contract"},{"local_two",apply.([x]),"both",nil},{"local_external",apply.([x]),"both_external",nil},{"local_anon",apply.([x]),"anonymous",nil},{"local_opacity",apply.([x]),"both","opacity"},{"local_reverse",apply.([x]),"reverse",nil},{"argument_side_effect",:cerl.ann_c_apply([1],:cerl.c_seq(:cerl.c_primop(lit.(:match_fail),[]),op),[:cerl.c_cons_skel(x,y)]),"fallback",nil},{"record_generated",:cerl.ann_c_call([:compiler_generated],lit.(:erlang),lit.(:is_record),[:cerl.ann_c_var([:compiler_generated],:x),lit.(:tag),lit.(2)]),"remote",nil}]
# A single typed state is shared by every reached argument and operator traversal.
rows=for {id,body,mode,profile}<-shapes,{xtype,i}<-Enum.with_index(pool),{other,j}<-Enum.with_index(if(mode in ["fallback","external"],do: op_pool,else: pool)),warnings<-[false,true],seed_mode<-(if mode in ["alpha","both","reverse","both_external","anonymous"],do: ["seeded","unhandled","none"],else: ["seeded"]) do
 name=:call_fixture;ev=v.({:entry,5});av=v.({:alpha,1});bv=v.({:beta,1});anon=:cerl.c_fun([z],lit.(:anonymous));entry=:cerl.c_fun([m,f,x,y,op],:cerl.c_tuple_skel([body,anon]));tree=:cerl.c_module(lit.(name),[ev],[],[{ev,entry},{av,:cerl.c_fun([z],lit.(:alpha))},{bv,:cerl.c_fun([z],lit.(:beta))}]);exported=CoreExport.export_module(name,[],tree);{tree,_}=:cerl_trees.label(tree,0);defs=:cerl.module_defs(tree);[{_,entry},{_,af},{_,bf}]=defs;[body,anon]=:cerl.tuple_es(:cerl.fun_body(entry));params=:cerl.fun_vars(entry);al=:cerl_trees.get_label(af);bl=:cerl_trees.get_label(bf);nl=:cerl_trees.get_label(anon)
 cg=:dialyzer_callgraph.new();plt=:dialyzer_plt.new();cs=:dialyzer_codeserver.new();cgget=fn key->FunctionStateOracle.get(cg,cgfs,key)end
 try do
  :ets.insert(cgget.(:esc),{:cerl_trees.get_label(entry)})
  for {variable,function}<-defs do
   {n,a}=:cerl.var_name(variable);label=:cerl_trees.get_label(function);mfa={name,n,a};:ets.insert(cgget.(:name_map),{label,mfa});:ets.insert(cgget.(:rev_name_map),{mfa,label});:ets.insert(cgget.(:letrec_map),{:cerl_trees.get_label(variable),label})
  end
  locals=[%{label: al,signature: sig.([k.("integer")],atom.(:alpha))},%{label: bl,signature: sig.([k.("atom")],atom.(:beta))},%{label: nl,signature: sig.([k.("integer")],atom.(:anonymous))}]
  locals=if profile=="opacity",do: [ %{label: al,signature: sig.([nom.(k.("integer"))],atom.(:alpha))}|tl(locals)],else: locals
  for r<-locals do
   key=case :dialyzer_callgraph.lookup_name(r.label,cg) do {:ok,mfa}->mfa;:error->r.label end
   :dialyzer_plt.insert_list(plt,[{key,{TypeCodec.type(r.signature.result),Enum.map(r.signature.arguments,&TypeCodec.type/1)}}])
  end
  remote=case profile do
   nil->[];"sig_bif_abs"->[%{mfa: %{module: "erlang",function: "abs",arity: 1},signature: sig.([k.("atom")],k.("atom"))}];"opacity"->[%{mfa: %{module: "library",function: "typed",arity: 1},signature: sig.([nom.(k.("integer"))],atom.(:ok))}];"range"->[%{mfa: %{module: "library",function: "typed",arity: 1},signature: sig.([k.("integer")],atom.(:ok))}];_->[%{mfa: %{module: "library",function: "typed",arity: 1},signature: sig.([k.("integer")],k.("any"))}]
  end
  for r<-remote,do: :dialyzer_plt.insert_list(plt,[{{String.to_atom(r.mfa.module),String.to_atom(r.mfa.function),r.mfa.arity},{TypeCodec.type(r.signature.result),Enum.map(r.signature.arguments,&TypeCodec.type/1)}}])
  contract_specs=case profile do
   "local_contract"->[{"(integer()) -> integer()",fun.([k.("integer")],k.("integer")),[]}];"contract"->[{"(integer()) -> integer()",fun.([k.("integer")],k.("integer")),[]}];"range"->[{"(integer()) -> integer()",fun.([k.("integer")],k.("integer")),[]}];"overload"->[{"(integer()) -> integer()",fun.([k.("integer")],k.("integer")),[]},{"(atom()) -> atom()",fun.([k.("atom")],k.("atom")),[]}];"bounded"->[{"(A) -> A when A :: integer()",fun.([%{"kind"=>"var","id"=>%{"atom"=>"A"}}],%{"kind"=>"var","id"=>%{"atom"=>"A"}}),[[%{"kind"=>"var","id"=>%{"atom"=>"A"}},k.("integer")]]}];_->[]
  end
  contracts=if contract_specs==[],do: [],else: (
   parts=Enum.map(contract_specs,fn {_,ft,cs}->%{function: ft,constraints: cs}end);native_parts=Enum.map(parts,fn p->{TypeCodec.type(p.function),Enum.map(p.constraints,fn [a,b]->{:subtype,TypeCodec.type(a),TypeCodec.type(b)}end)}end)
   general=:erl_types.t_sup(Enum.map(native_parts,fn {ft,cs}->:erl_types.t_subst(ft,Map.new(cs,fn {:subtype,a,b}->{:erl_types.t_var_name(a),b}end))end))|>:erl_types.t_subst(%{})
   source="-spec f"<>(contract_specs|>Enum.map(&elem(&1,0))|>Enum.join(";"))<>"."
   {:ok,tokens,_}=:erl_scan.string(String.to_charlist(source));{:ok,{:attribute,_,:spec,{_,formlist}}}=:erl_parse.parse_form(tokens)
   forms=Enum.map(formlist,fn {:type,_,:bounded_fun,[ft,cs]}->{ft,cs};ft->{ft,[]}end)
   c={:contract,native_parts,:erl_types.t_fun_args(general),forms};mfa=if(profile=="local_contract",do: {:call_fixture,:alpha,1},else: {:library,:typed,1});:dialyzer_plt.insert_contract_list(plt,[{mfa,c}]);[%{mfa: %{module: Atom.to_string(elem(mfa,0)),function: Atom.to_string(elem(mfa,1)),arity: 1},alternatives: parts,capture: CoreExport.export_terms(c)}]
  )
  targets=case mode do "fallback"->nil;"external"->[:external];"alpha"->[al];"both"->[al,bl];"reverse"->[bl,al];"both_external"->[al,bl,:external];"anonymous"->[nl];_->nil end
  if targets,do: :ets.insert(cgget.(:calls),{:cerl_trees.get_label(body),targets})
  state=:rust_function_native.state__new(cg,cs,:cerl.ann_c_fun([{:label,:top}],[],tree),plt,name,%{})
  seeds=for {label,result}<- [{al,atom.(:alpha)},{bl,atom.(:beta)},{nl,atom.(:anonymous)}] do
   args=[k.("any")];fun=case label do ^al->af;^bl->bf;^nl->anon end
   %{label: label,arguments: args,result: result,fun: fun}
  end
  seeds=case seed_mode do "unhandled"->[];"none"->Enum.map(seeds,fn s->%{s|arguments: [k.("none")],result: k.("none")}end);_->seeds end
  state=Enum.reduce(seeds,state,fn s,state->state=:rust_function_native.forward_args(s.label,Enum.map(s.arguments,&TypeCodec.type/1),state);:rust_function_native.state__update_fun_entry(s.fun,Enum.map(s.arguments,&TypeCodec.type/1),TypeCodec.type(s.result),state)end)
  clear=fn clear,s->case :rust_function_native.state__get_work(s) do :none->s;{_,s}->clear.(clear,s)end end;state=clear.(clear,state)
  state=if warnings,do: :rust_function_native.state__set_warning_mode(state),else: state
  state=set.(state,:curr_fun,{name,:entry,5})
  actual=case mode do "dynamic"->[xtype,other,k.("integer"),k.("any"),k.("any")];"fallback"->[atom.(:erlang),atom.(:f),xtype,k.("integer"),other];"external"->[atom.(:erlang),atom.(:f),xtype,k.("integer"),other];_->[atom.(:erlang),atom.(:f),xtype,other,fun.([k.("any")],k.("any"))]end
  map=:rust_function_native.enter_type_lists(params,Enum.map(actual,&TypeCodec.type/1),:rust_function_native.map__new())
  {native,warns}=try do
   {state,map,t}=:rust_function_native.traverse(body,map,state)
   {%{status: "ok",type: NativeTypeWire.encode(t),map: mapwire.(map),functions: FunctionStateOracle.snap(state,fields),warnings_match: true},get.(state,:warnings)}
  catch kind,reason->{%{status: "error",kind: Atom.to_string(kind),reason: CoreExport.export_terms(reason)},[]} end
  %{id: "#{id}:#{i}:#{j}:#{warnings}:#{seed_mode}",program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},arguments: actual,warning_mode: warnings,escaping: [:cerl_trees.get_label(entry)],local_signatures: locals,seeds: Enum.map(seeds,&Map.delete(&1,:fun)),remote_signatures: remote,contracts: contracts,targets: targets,native: native,warnings: CoreExport.export_terms(warns)}
 after :dialyzer_callgraph.delete(cg);:dialyzer_plt.delete(plt);:dialyzer_codeserver.delete(cs)end
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]));IO.inspect(%{cases: length(rows)})
