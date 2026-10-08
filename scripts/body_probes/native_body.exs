[root,out|options]=System.argv()
suite=List.first(options)||"structural"
Code.require_file(Path.join(root,"scripts/type_codec.exs"))
Code.require_file(Path.join(root,"scripts/type_wire.exs"))
Code.require_file(Path.join(root,"scripts/core_export.exs"))
source=File.read!(Path.join(root,"scripts/dataflow_state_probes/function_state_oracle.exs"))|>String.split("defmodule FunctionStateOracle")|>List.last()|>String.split("fields=FunctionStateOracle.load()")|>hd()
Code.eval_string("defmodule FunctionStateOracle"<>source)
{fields,cgfs}=FunctionStateOracle.load()
get=fn s,k->FunctionStateOracle.get(s,fields,k)end
set=fn s,k,v->put_elem(s,Enum.find_index(fields,&(&1==k))+1,v)end
{:ok,{_,[abstract_code: {:raw_abstract_v1,forms}]}}=:beam_lib.chunks(:code.which(:dialyzer_dataflow),[:abstract_code])
{:attribute,_,:record,{:map,mapfs}}=Enum.find(forms,&match?({:attribute,_,:record,{:map,_}},&1));mapfs=Enum.map(mapfs,fn f->f=if elem(f,0)==:typed_record_field,do: elem(f,1),else: f;elem(elem(f,2),2)end)
mapget=fn m,k->elem(m,Enum.find_index(mapfs,&(&1==k))+1)end
mapwire=fn m->%{types: Map.new(mapget.(m,:map),fn {k,v}->{Integer.to_string(k),NativeTypeWire.encode(v)}end),substitutions: Map.new(mapget.(m,:subst),fn {k,v}->{Integer.to_string(k),if(is_integer(v),do: v,else: :cerl_trees.get_label(v))}end),modified: mapget.(m,:modified)}end
lit=&:cerl.abstract/1
v=&:cerl.ann_c_var([1],&1)
x=v.(:x);y=v.(:y);z=v.(:z);fail=:cerl.c_primop(lit.(:match_fail),[])
leaves=[:a,0,1,1.0,[],{},[1,2],[1|:tail],%{},<<3::3>>]
base=Enum.flat_map(leaves,fn a->[lit.(a),:cerl.c_tuple_skel([lit.(a),x]),:cerl.c_cons_skel(lit.(a),y),:cerl.c_cons_skel(x,lit.([a])),:cerl.c_let([z],lit.(a),:cerl.c_tuple_skel([z,x])),:cerl.c_seq(lit.(a),x)]end)
base=base++[x,y,z,:cerl.c_values([x,y]),:cerl.c_values([]),:cerl.c_alias(z,:cerl.c_tuple_skel([x,y])),fail,:cerl.c_catch(fail),:cerl.c_fun([y],:cerl.c_tuple_skel([x,y])),:cerl.c_letrec([{v.({:local,1}),:cerl.c_fun([y],:cerl.c_cons_skel(x,y))}],v.({:local,1})),:cerl.c_let([x,y],:cerl.c_values([z,z]),:cerl.c_tuple_skel([x,y])),:cerl.c_let([z],x,:cerl.c_tuple_skel([z,x])),:cerl.c_seq(x,y),:cerl.c_catch(:cerl.c_let([z],x,z))]
tries=for arg<-[x,fail,:cerl.c_values([x,y]),:cerl.c_fun([],x)],body<-[z,fail,:cerl.c_tuple_skel([x,z])],handler<-[v.(:reason),lit.(:caught),fail],do: :cerl.c_try(arg,[z],body,[v.(:kind),v.(:reason),v.(:trace)],handler)
base=base++tries++Enum.map([:build_stacktrace,:recv_peek_message,:bs_init_writable,:raise,:remove_message,:dialyzer_unknown,:nif_start,:debug_line,:executable_line],&:cerl.c_primop(lit.(&1),[]))++Enum.map([x,lit.(:infinity),fail],&:cerl.c_primop(lit.(:recv_wait_timeout),[&1]))
if suite not in ["structural","maps","binaries","loop","cases","case_records","calls_nested","module_calls","case_infix"],do: raise("unknown body suite #{suite}")
map_pairs=[[],[:cerl.c_map_pair(lit.(:a),y)],[:cerl.c_map_pair_exact(lit.(:a),y)],[:cerl.c_map_pair_exact(x,y)],[:cerl.c_map_pair(x,y)],[:cerl.c_map_pair(x,y),:cerl.c_map_pair_exact(lit.(:a),z)],[:cerl.c_map_pair(lit.(:a),y),:cerl.c_map_pair_exact(lit.(:a),z)],[:cerl.c_map_pair_exact(lit.(:a),y),:cerl.c_map_pair_exact(lit.(:b),z)],[:cerl.c_map_pair_exact(lit.(:a),fail),:cerl.c_map_pair(lit.(:b),:cerl.c_fun([],x))],[:cerl.c_map_pair(lit.(:a),:cerl.c_let([z],y,z)),:cerl.c_map_pair_exact(lit.(:b),z)]]
map_pairs=Enum.map(map_pairs,fn pairs->Enum.map(pairs,&:cerl.set_ann(&1,[1]))end)
base=if suite=="maps",do: (for arg<-[x,lit.(%{}),lit.(%{a: 1,b: :ok})],pairs<-map_pairs,do: :cerl.c_map(arg,pairs)),else: base
segments=for kind<-[:integer,:float,:binary],size<-[lit.(-1),lit.(0),lit.(8),y],unit<-[1,8],do: :cerl.c_bitstr(x,size,lit.(unit),lit.(kind),lit.([:unsigned,:big]))
segments=segments++[:cerl.c_bitstr(x,lit.(:all),lit.(8),lit.(:binary),lit.([:unsigned,:big]))]++Enum.map([:utf8,:utf16,:utf32],&:cerl.c_bitstr(x,lit.(:undefined),lit.(1),lit.(&1),lit.([:unsigned,:big])))
base=if suite=="binaries",do: [:cerl.c_binary([])]++Enum.flat_map(segments,fn segment->[segment,:cerl.c_binary([segment]),:cerl.c_binary([segment,:cerl.c_bitstr(y,lit.(1),lit.(1),lit.(:integer),lit.([:unsigned,:big]))])]end),else: base
base=if suite in ["cases","case_infix"] do
 pats=[v.(:p),lit.(0),lit.(:a),lit.([]),:cerl.c_cons_skel(v.(:p),v.(:q)),:cerl.c_tuple_skel([lit.(:tag),v.(:p)]),:cerl.c_alias(v.(:p),:cerl.c_tuple_skel([lit.(:tag),v.(:q)])),:cerl.c_map_pattern([:cerl.c_map_pair_exact(lit.(:a),v.(:p))]),lit.(%{a: 0})]
 guards=[lit.(true),lit.(false),:cerl.c_call(lit.(:erlang),lit.(:is_atom),[v.(:p)]),:cerl.c_call(lit.(:erlang),lit.(:is_integer),[v.(:p)])]
 guards=if suite=="case_infix",do: (for op<-[:==,:"=:=",:<,:"=/="],do: :cerl.c_call(lit.(:erlang),lit.(op),[v.(:p),lit.(0)])),else: guards
 bodies=[lit.(:yes),v.(:p),fail]
 patterns=for pat<-pats,guard<-guards,body<-bodies do
 bound=:p in Enum.map(:cerl.pat_vars(pat),&:cerl.var_name/1)
 guard=if bound,do: guard,else: :cerl_trees.map(fn n->if :cerl.type(n)==:var and :cerl.var_name(n)==:p,do: z,else: n end,guard)
 body=if bound,do: body,else: :cerl_trees.map(fn n->if :cerl.type(n)==:var and :cerl.var_name(n)==:p,do: z,else: n end,body)
 :cerl.c_case(x,[:cerl.c_clause([pat],guard,body),:cerl.c_clause([v.(:rest)],lit.(true),lit.(:no))])
 end
 patterns=patterns++[:cerl.c_case(x,[]),:cerl.c_case(:cerl.c_values([x,y]),[:cerl.c_clause([v.(:p),v.(:q)],lit.(true),:cerl.c_tuple_skel([v.(:p),v.(:q)]))]),:cerl.c_case(x,[:cerl.c_clause([lit.(0)],lit.(true),lit.(:zero)),:cerl.c_clause([lit.(0)],lit.(true),lit.(:covered))])]
 Enum.map(patterns,fn node->:cerl_trees.map(fn child->:cerl.set_ann(child,[1])end,node)end)
else
 base
end
base=if suite in ["calls_nested","module_calls"] do
 call=fn m,f,args->:cerl.c_call(lit.(m),lit.(f),args)end
 apply=fn args->:cerl.c_apply(:cerl.c_fun([v.(:n)],v.(:n)),args)end
 inner=call.(:erlang,:abs,[x])
 [ inner,call.(:erlang,:abs,[inner]),call.(:erlang,:length,[call.(:erlang,:atom_to_list,[x])]),
   call.(:erlang,:is_integer,[inner]),call.(:maps,:get,[y,call.(:maps,:put,[y,x,lit.(%{})])]),
   call.(:unknown,:missing,[inner,call.(:erlang,:length,[y])]),
   :cerl.c_seq(inner,call.(:erlang,:length,[y])),
   :cerl.c_let([z],inner,call.(:erlang,:abs,[z])),
   :cerl.c_case(x,[:cerl.c_clause([v.(:p)],lit.(true),call.(:erlang,:abs,[v.(:p)]))]),
   :cerl.c_case(inner,[:cerl.c_clause([v.(:p)],lit.(true),v.(:p))]),
   apply.([inner]),:cerl.c_apply(:cerl.c_fun([],lit.(:ok)),[]),
   :cerl.c_call(fail,lit.(:abs),[:cerl.c_fun([],x)]),
   :cerl.c_call(lit.(:erlang),fail,[:cerl.c_fun([],x)]),
   :cerl.c_apply(:cerl.c_fun([v.(:n)],v.(:n)),[fail]),
   :cerl.c_apply(fail,[:cerl.c_fun([],x)])
 ] |> Enum.map(fn node->:cerl_trees.map(fn n->:cerl.set_ann(n,[1])end,node)end)
else base end
base=if suite=="module_calls",do: base++[:cerl_trees.map(fn n->:cerl.set_ann(n,[1])end,:cerl.c_fun([],:cerl.c_cons_skel(x,lit.(:bad_tail))))],else: base
base=if suite=="case_records" do
 pat=:cerl.ann_c_tuple_skel([1,:record],[lit.(:tag),v.(:p)])
 guard=:cerl.c_call(lit.(:erlang),lit.(:is_record),[x,lit.(:tag),lit.(2)])
 [ :cerl.c_case(x,[:cerl.c_clause([v.(:p)],guard,v.(:p))]),
   :cerl.c_case(x,[:cerl.c_clause([pat],lit.(true),v.(:p))]),
   :cerl.c_case(x,[:cerl.c_clause([v.(:p)],lit.(true),lit.(:yes)),:cerl.c_clause([pat],lit.(true),v.(:p))])
 ] |> Enum.map(fn node->:cerl_trees.map(fn n->:cerl.set_ann(n,Enum.uniq([1|:cerl.get_ann(n)]))end,node)end)
else base end
records=if suite=="case_records",do: [%{"tag"=>"tag","fields"=>[%{"kind"=>"integer"}]}],else: []
record_table=Map.new(records,fn record->fs=Enum.with_index(record["fields"],fn type,i->{String.to_atom("field#{i}"),{:type,0,:any,[]},TypeCodec.type(type)}end);{{:record,String.to_atom(record["tag"])},{{~c"body_fixture",1},[{length(fs),fs}]}}end)
base=Enum.map(base,&:cerl.set_ann(&1,[1]))
base=if System.get_env("BODY_SHAPE_ID"),do: [Enum.at(base,String.to_integer(System.get_env("BODY_SHAPE_ID")))],else: base
pool=Enum.map(["any","none","unit","integer","atom","float","nil","tuple_any","map_any"],&%{"kind"=>&1})++[%{"kind"=>"atoms","values"=>["true","false"]},%{"kind"=>"tuple","elements"=>[%{"kind"=>"atom"},%{"kind"=>"integer"}]}]
pool=if suite=="maps",do: pool++[%{"kind"=>"atoms","values"=>["a"]},%{"kind"=>"integers","values"=>[0,1]},%{"kind"=>"map","entries"=>[%{"key"=>%{"kind"=>"atoms","values"=>["a"]},"presence"=>"mandatory","value"=>%{"kind"=>"integer"}}],"default_key"=>%{"kind"=>"none"},"default_value"=>%{"kind"=>"none"}}],else: pool
pool=if suite=="binaries",do: pool++[%{"kind"=>"binary"},%{"kind"=>"bitstring","unit"=>1,"base"=>0},%{"kind"=>"integers","values"=>[0,1,7]},%{"kind"=>"range","from"=>-1,"to"=>7},%{"kind"=>"nominal","name"=>%{"module"=>"outside","name"=>"size","arity"=>0,"opacity"=>"opaque"},"inner"=>%{"kind"=>"integer"}}],else: pool
pool=if suite in ["cases","case_infix"],do: pool++[%{"kind"=>"atoms","values"=>["a"]},%{"kind"=>"integers","values"=>[0,1]},%{"kind"=>"binary"},%{"kind"=>"list","elements"=>%{"kind"=>"integer"},"tail"=>%{"kind"=>"nil"},"nonempty"=>true},%{"kind"=>"map","entries"=>[%{"key"=>%{"kind"=>"atoms","values"=>["a"]},"presence"=>"mandatory","value"=>%{"kind"=>"integer"}}],"default_key"=>%{"kind"=>"none"},"default_value"=>%{"kind"=>"none"}}]++(for {name,inner}<-[{"number",%{"kind"=>"integer"}},{"tuple",%{"kind"=>"tuple","elements"=>[%{"kind"=>"atoms","values"=>["tag"]},%{"kind"=>"integer"}]}},{"map",%{"kind"=>"map_any"}}],do: %{"kind"=>"nominal","name"=>%{"module"=>"outside","name"=>name,"arity"=>0,"opacity"=>"opaque"},"inner"=>inner}),else: pool
pool=if suite=="case_records",do: pool++[%{"kind"=>"tuple","elements"=>[%{"kind"=>"atoms","values"=>["tag"]},%{"kind"=>"atoms","values"=>["wrong"]}]},%{"kind"=>"tuple","elements"=>[%{"kind"=>"atoms","values"=>["tag"]},%{"kind"=>"integer"}]}],else: pool
pool=if suite in ["loop","module_calls"],do: [%{"kind"=>"any"}],else: pool
ypool=if suite=="binaries",do: Enum.reject(pool,&(&1["kind"]=="unit")),else: pool
rows=for {body,id}<-Enum.with_index(base),{xtype,i}<-Enum.with_index(pool),{ytype,j}<-Enum.with_index(ypool),warning<-[false,true] do
 if i==0 and j==0 and not warning,do: IO.puts(:stderr,"#{suite} shape #{id}/#{length(base)}")
 if path=System.get_env("BODY_TRACE"),do: File.write!(path,:json.encode(%{shape: id,x: xtype,y: ytype,warning: warning}))
 name=:body_fixture;variable=v.({:entry,3});fun=:cerl.c_fun([x,y,z],:cerl.c_tuple_skel([body]));tree=:cerl.c_module(lit.(name),[variable],[],[{variable,fun}]);exported=CoreExport.export_module(name,[],tree)
 {tree,_}=:cerl_trees.label(tree,0);[{_,fun}]=:cerl.module_defs(tree);[xv,yv,zv]=:cerl.fun_vars(fun)
 cg=:dialyzer_callgraph.new();plt=:dialyzer_plt.new();cs=:dialyzer_codeserver.new()
 try do
  cgget=fn key->FunctionStateOracle.get(cg,cgfs,key)end
  :cerl_trees.fold(fn node,acc->
   if :cerl.is_c_fun(node),do: :ets.insert(cgget.(:esc),{:cerl_trees.get_label(node)})
   defs=case :cerl.type(node) do :module->:cerl.module_defs(node);:letrec->:cerl.letrec_defs(node);_->[]end
   for {var,fun}<-defs,do: :ets.insert(cgget.(:letrec_map),{:cerl_trees.get_label(var),:cerl_trees.get_label(fun)})
   acc
  end,:ok,tree)
  for {variable,fun}<-:cerl.module_defs(tree) do
   {f,a}=:cerl.var_name(variable);label=:cerl_trees.get_label(fun);mfa={name,f,a}
   :ets.insert(cgget.(:name_map),{label,mfa});:ets.insert(cgget.(:rev_name_map),{mfa,label})
  end
  state=:rust_function_native.state__new(cg,cs,:cerl.ann_c_fun([{:label,:top}],[],tree),plt,name,record_table)
  state=if warning and suite not in ["loop","module_calls"],do: :rust_function_native.state__set_warning_mode(state),else: state
  state=set.(state,:curr_fun,{name,:entry,3})
  map=:rust_function_native.map__new()|>then(&:rust_function_native.enter_type_lists([xv,yv,zv],[TypeCodec.type(xtype),TypeCodec.type(ytype),:any],&1))
  {native,warnings}=try do
   {state,map,type}=if suite in ["loop","module_calls"] do
    state=:rust_function_native.analyze_loop(state)
    state=if warning,do: :rust_function_native.analyze_loop(:rust_function_native.state__set_warning_mode(state)),else: state
    {state,map,:any}
   else
    :rust_function_native.traverse(hd(:cerl.tuple_es(:cerl.fun_body(fun))),map,state)
   end
   {%{status: "ok",type: NativeTypeWire.encode(type),map: mapwire.(map),functions: FunctionStateOracle.snap(state,fields),warnings_match: true},get.(state,:warnings)}
  catch :error,{:case_clause,tag} when is_atom(tag)->{%{status: "error",kind: "case_clause",payload: Atom.to_string(tag)},[]};:error,:function_clause->{%{status: "error",kind: "function_clause"},[]};kind,reason->raise "unclassified body #{inspect({id,kind,reason})}" end
  %{id: "#{id}/#{i}/#{j}/#{warning}",records: records,program: %{version: 6,profile: CoreExport.profile(),modules: [exported]},arguments: [xtype,ytype,%{kind: "any"}],warning_mode: warning,fixture_escaping: "all",module_loop: suite in ["loop","module_calls"],native: native,warnings: CoreExport.export_terms(warnings)}
 after :dialyzer_callgraph.delete(cg);:dialyzer_plt.delete(plt);:dialyzer_codeserver.delete(cs)end
end
File.write!(out,Enum.map(rows,&[:json.encode(&1),"\n"]))
IO.inspect(%{cases: length(rows),shapes: length(base)})
