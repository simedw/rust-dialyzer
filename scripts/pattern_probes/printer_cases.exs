Code.require_file("../core_export.exs", __DIR__)
v=:cerl.c_var(:X);y=:cerl.c_var(:Y);a=:cerl.c_atom(:a);i=:cerl.c_int(1)
clauses=[ :cerl.c_clause([a],:cerl.c_atom(true),i),:cerl.c_clause([v],:cerl.c_atom(true),v)]
case1=:cerl.c_case(v,[hd(clauses)]);case2=:cerl.c_case(v,clauses)
base=[{:atom,a},{:int,i},{:float,:cerl.c_float(1.25)},{:var,v},{:string,:cerl.abstract(~c"hello")},{:unicode,:cerl.abstract(~c"你好")},{:improper,:cerl.abstract([1|2])},{:maplit,:cerl.abstract(%{1=>2,1.0=>3,a: <<1,2>>})},{:mapupdate,:cerl.c_map(v,[:cerl.c_map_pair(a,i)])},{:mapempty,:cerl.c_map([])},{:case1,case1},{:case2,case2},{:tuple_case,:cerl.c_tuple([i,case2])},{:fun_case,:cerl.c_fun([v],case2)},{:let_case,:cerl.c_let([v],i,case2)},{:seq_case,:cerl.c_seq(i,case2)},{:apply,:cerl.c_apply(v,[i,a])},{:call,:cerl.c_call(a,a,[i,v])},{:primop,:cerl.c_primop(a,[i,v])},{:receive,:cerl.c_receive(clauses,i,v)},{:try,:cerl.c_try(case2,[v],y,[v,y],i)},{:catch,:cerl.c_catch(case2)},{:letrec,:cerl.c_letrec([{:cerl.c_var({:f,0}),:cerl.c_fun([],case2)}],v)},{:bits,:cerl.abstract(<<1,2,3::size(2)>>)},{:binary,:cerl.c_binary([:cerl.c_bitstr(v,:cerl.c_int(3),:cerl.c_int(2),:cerl.c_atom(:integer),:cerl.abstract([:little,:signed]))])}]
for {name,expr}<-base, mode<-0..2 do
 expr=case mode do
  0->expr
  1->:cerl_trees.map(fn n->:cerl.set_ann(n,[{:line,9},1]) end,expr)
  2->:cerl.c_tuple([:cerl.ann_c_var([4],:X),:cerl.set_ann(expr,[3])])
 end
 root=:cerl.c_module(:cerl.c_atom(:fixture),[],[],[{:cerl.c_var({:fixture,0}),:cerl.c_fun([],expr)}])
 mod=CoreExport.export_module(:fixture,[],root);[{_,fun}]=:cerl.module_defs(root)
 # exported IDs are postorder; resolve body by its function node groups.
 fn_node=Enum.find(mod.nodes,&(&1.kind=="fun" && &1.groups==[[],[mod.core-1]]))
 _=fn_node
 module_node=Enum.at(mod.nodes,mod.core);[[name_id],_,_,defs]=module_node.groups
 _=name_id
 [_var,fun_id]=defs
 fun_node=Enum.at(mod.nodes,fun_id);[_,[body]]=fun_node.groups
 IO.puts(:json.encode(%{id: "#{name}-#{mode}",core: %{version: 5,profile: CoreExport.profile(),modules: [mod]},node: body,native: List.to_string(:cerl_prettypr.format(:cerl.set_ann(:cerl.fun_body(fun),[]),[{:hook,:dialyzer_utils.pp_hook()},:noann,{:paper,100000},{:ribbon,100000}]))}))
end
