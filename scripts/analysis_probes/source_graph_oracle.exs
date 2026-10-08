Code.require_file("compiler_atom_primitives.exs",__DIR__)
:logger.set_primary_config(:level,:none)
atoms=[:top,:m,:f,:g,:in,:out,:"$e"]++Enum.map(0..4499,&String.to_atom("module_#{&1}"))
profile=CompilerAtomPrimitives.export(atoms,"source-graph-proof-1")
encode=fn encode,t->cond do is_atom(t)->%{atom: Atom.to_string(t)};is_integer(t)->%{integer: Integer.to_string(t)};is_tuple(t)->%{tuple: t|>Tuple.to_list()|>Enum.map(&encode.(encode,&1))};t==[]->%{nil: true};true->[h|tail]=t;%{cons: [encode.(encode,h),encode.(encode,tail)]}end end
snapshot=fn graph,roots->
 vs=:digraph.vertices(graph)
 scc=:digraph_utils.strong_components(graph)
 sub=:digraph_utils.subgraph(graph,:digraph_utils.reaching(roots,graph))
 subdata=%{vertices: Enum.map(:digraph.vertices(sub),&encode.(encode,&1)),edges: Enum.map(:digraph.edges(sub),&encode.(encode,&1)),scc: Enum.map(:digraph_utils.strong_components(sub),fn c->Enum.map(c,&encode.(encode,&1)) end)}
 :digraph.delete(sub)
 %{vertices: Enum.map(vs,&encode.(encode,&1)),edges: Enum.map(:digraph.edges(graph),&encode.(encode,&1)),adjacency: Enum.map(vs,fn v->%{vertex: encode.(encode,v),in: Enum.map(:digraph.in_neighbours(graph,v),&encode.(encode,&1)),out: Enum.map(:digraph.out_neighbours(graph,v),&encode.(encode,&1)),in_edges: Enum.map(:digraph.in_edges(graph,v),&encode.(encode,&1)),out_edges: Enum.map(:digraph.out_edges(graph,v),&encode.(encode,&1))} end),scc: Enum.map(scc,fn c->Enum.map(c,&encode.(encode,&1)) end),reaching: Enum.map(:digraph_utils.reaching(roots,graph),&encode.(encode,&1)),subgraph: subdata}
end
for shape<-0..2,n<-[0,1,2,10,33,64,257,513,2305,4500],seed<-0..2 do
 keys=Enum.map(Enum.take(Stream.iterate(0,&(&1+1)),n),fn i->case shape do 0->i;1->{String.to_atom("module_#{i}"),:f,rem(i,3)};2->if rem(i,2)==0,do: i,else: {:m,:g,i} end end)
 keys=if rem(seed,2)==0,do: keys,else: Enum.reverse(keys)
 confirmed=Enum.reject(keys,fn v->rem(:erlang.phash2(v)+seed,7)==0 end)
 edges=if n==0,do: [],else: Enum.map(0..(n*3-1),fn i->{Enum.at(keys,rem(i*17+seed,n)),Enum.at(keys,rem(i*i+13*i+seed*3,n))} end)
 graph=:digraph.new()
 Enum.each(confirmed,&:digraph.add_vertex(graph,&1,:confirmed))
 Enum.each(edges,fn {a,b}->
   Enum.each([a,b],fn v->if :digraph.vertex(graph,v)==false,do: :digraph.add_vertex(graph,v) end)
   :digraph.add_edge(graph,{a,b},a,b,[])
 end)
 roots=Enum.take(confirmed,3)
 before=snapshot.(graph,roots)
 unconfirmed=:digraph.vertices(graph)|>Enum.filter(fn v->case :digraph.vertex(graph,v) do {_,:confirmed}->false;{_,[]}->true end end)|>Enum.reverse()
 :digraph.del_vertices(graph,unconfirmed)
 after_=snapshot.(graph,roots)
 :digraph.delete(graph)
 IO.puts(:json.encode(%{id: [shape,n,seed],profile: profile,confirmed: Enum.map(confirmed,&encode.(encode,&1)),edges: Enum.map(edges,&encode.(encode,&1)),roots: Enum.map(roots,&encode.(encode,&1)),before: before,after: after_}))
end
