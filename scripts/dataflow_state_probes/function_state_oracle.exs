Code.require_file("type_codec.exs",__DIR__)
Code.require_file("type_wire.exs",__DIR__)
defmodule FunctionStateOracle do
 def load do
  {:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_dataflow),[:abstract_code])
  {:attribute,_,:record,{:state,fields}}=Enum.find(forms,&match?({:attribute,_,:record,{:state,_}},&1))
  names=Enum.map(fields,fn f->f=if elem(f,0)==:typed_record_field,do: elem(f,1),else: f;elem(elem(f,2),2) end)
  {:ok,tokens,_}=:erl_scan.string(~c"state_fixture(Tab,Trees,Plt,CG,Work,Envs) -> #state{fun_tab=Tab,tree_map=Trees,plt=Plt,callgraph=CG,work=Work,envs=Envs,warning_mode=false,reachable_funs=sets:new()}." )
  {:ok,wrapper}=:erl_parse.parse_form(tokens)
  forms=Enum.map(forms,fn {:attribute,a,:module,_}->{:attribute,a,:module,:rust_function_native};f->f end)++[wrapper]
  {:ok,:rust_function_native,bin,_}=:compile.forms(forms,[:binary,:export_all,:return_warnings]);{:module,:rust_function_native}=:code.load_binary(:rust_function_native,~c"state_oracle",bin)
  {:ok,{_,[abstract_code: {:raw_abstract_v1,cgforms}]}}=:beam_lib.chunks(:code.which(:dialyzer_callgraph),[:abstract_code])
  {:attribute,_,:record,{:callgraph,cgfs}}=Enum.find(cgforms,&match?({:attribute,_,:record,{:callgraph,_}},&1))
  cgfs=Enum.map(cgfs,fn f->f=if elem(f,0)==:typed_record_field,do: elem(f,1),else: f;elem(elem(f,2),2) end)
  {names,cgfs}
 end
 def get(record,names,name),do: elem(record,Enum.find_index(names,&(&1==name))+1)
 def key("top"),do: :top
 def key(k),do: k
 def snap(state,names) do
  entries=Map.new(:dict.to_list(get(state,names,:fun_tab)),fn {key,entry}->
   {called,{args,ret}}=case entry do {:not_handled,signature}->{false,signature};signature->{true,signature} end
   {to_string(key),%{called: called,args: Enum.map(args,&NativeTypeWire.encode/1),result: NativeTypeWire.encode(ret)}}
  end)
  work=get(state,names,:work)
  %{entries: entries,environments: Map.new(:dict.to_list(get(state,names,:envs)),fn {key,map}->{to_string(key),Map.new(elem(map,1),fn {k,v}->{to_string(k),NativeTypeWire.encode(v)}end)}end),queue: elem(work,0)++Enum.reverse(elem(work,1)),warning_mode: get(state,names,:warning_mode),reachable: :sets.to_list(get(state,names,:reachable_funs))|>Enum.sort()}
 end
 def run(row,{names,cgfs}) do
  cg=:dialyzer_callgraph.new();plt=:dialyzer_plt.new();trees=Enum.reduce(row["definitions"],:dict.new(),fn d,tab->
   label=key(d["key"]);fun=:cerl.ann_c_fun([{:label,label}],for(i<-Enum.to_list(1..d["arity"]//1),do: :cerl.c_var(i)),:cerl.c_atom(:ok))
   if d["escaping"],do: :ets.insert(get(cg,cgfs,:esc),{label})
   if d["named"] do
    mfa={:fixture,String.to_atom("function_#{label}"),d["arity"]};:ets.insert(get(cg,cgfs,:name_map),{label,mfa});:ets.insert(get(cg,cgfs,:rev_name_map),{mfa,label})
   end
   raw=d["raw"]
   if raw not in [nil,:null] do
    k=case :dialyzer_callgraph.lookup_name(label,cg) do {:ok,mfa}->mfa;:error->label end
    :dialyzer_plt.insert_list(plt,[{k,{TypeCodec.type(raw),List.duplicate(:any,d["arity"])}}])
   end
   :dict.store(label,fun,tab)
  end)
  graph_key=fn key->case :dialyzer_callgraph.lookup_name(key,cg) do {:ok,name}->name;:error->key end end
  edges=Enum.map(row["edges"]||[],fn [from,to]->{graph_key.(from),graph_key.(to)}end)
  vertices=for d<-row["definitions"],d["key"] != "top",do: graph_key.(d["key"])
  :dialyzer_callgraph.add_edges(edges,vertices,cg)
  callers=Map.new(row["definitions"],fn d->
   key=key(d["key"])
   incoming=if key==:top do
    []
   else
    case :dialyzer_callgraph.in_neighbours(key,cg) do
     :none->[]
     xs->for x<-xs,{:ok,label}<-[ :dialyzer_callgraph.lookup_label(x,cg)],:dict.is_key(label,trees),do: label
    end
   end
   {to_string(key),incoming}
  end)
  order=:dict.fetch_keys(trees);tab=:rust_function_native.init_fun_tab(order,:dict.new(),trees,cg,plt)
  initial=Enum.filter(order,fn key->key != :top and :dialyzer_callgraph.is_escaping(key,cg) and :dialyzer_callgraph.lookup_name(key,cg) != :error end)
  envs=Enum.reduce(order,:dict.new(),fn key,d->:dict.store(key,:rust_function_native.map__new(),d) end)
  state=:rust_function_native.state_fixture(tab,trees,plt,cg,:rust_function_native.init_work(initial),envs)
  try do
   {steps,state}=Enum.map_reduce(row["operations"],state,fn op,state->
    key=key(op["key"]||"top")
    next=case op["kind"] do
     "pop"->case :rust_function_native.state__get_work(state) do :none->state;{_,s}->s end
     "warning"->:rust_function_native.state__set_warning_mode(state)
     "forward"->:rust_function_native.forward_args(key,Enum.map(op["args"],&TypeCodec.type/1),state)
     "return"->:rust_function_native.state__update_fun_entry(:dict.fetch(key,trees),Enum.map(op["args"],&TypeCodec.type/1),TypeCodec.type(op["result"]),state)
     "capture"->if get(state,names,:warning_mode),do: state,else: state|>then(fn s->:rust_function_native.state__add_work(key,s) end)|>then(fn s->:rust_function_native.state__update_fun_env(:dict.fetch(key,trees),Enum.reduce(op["environment"]||[],:rust_function_native.map__new(),fn [k,t],m->:rust_function_native.store_map(k,TypeCodec.type(t),m)end),s) end)|>then(fn s->:rust_function_native.state__add_reachable(key,s) end)
    end
    {snap(next,names),next}
   end)
   functions=Map.new(:rust_function_native.state__all_fun_types(state),fn {key,type}->{to_string(key),NativeTypeWire.encode(type)} end)
   %{id: row["id"],order: order,steps: steps,functions: functions,caller_facts: callers}
  after
   :dialyzer_callgraph.delete(cg);:dialyzer_plt.delete(plt)
  end
 end
end
fields=FunctionStateOracle.load()
for line<-IO.stream(:stdio,:line),do: IO.puts(:json.encode(FunctionStateOracle.run(:json.decode(line),fields)))
