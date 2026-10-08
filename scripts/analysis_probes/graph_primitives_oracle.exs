Code.require_file("../compiler_atom_primitives.exs",__DIR__)
:logger.set_primary_config(:level,:none)
previous=:erts_debug.set_internal_state(:available_internal_state,true)
atoms=[:a,:b,:c,:top,:external,:in,:out,:"$e",:m,:n,:f,:g,:null,:false,:true,:"ångström",:"测试"]
profile=CompilerAtomPrimitives.export(atoms,"primitive-proof-1")
encode=fn encode,term->cond do
 is_atom(term)->%{atom: Atom.to_string(term)}
 is_integer(term)->%{integer: Integer.to_string(term)}
 is_tuple(term)->%{tuple: term|>Tuple.to_list()|>Enum.map(&encode.(encode,&1))}
 is_list(term) and term==[]->%{nil: true}
 true->{head,tail}=case term do [head|tail]->{head,tail} end;%{cons: [encode.(encode,head),encode.(encode,tail)]}
end end
rows=(for atom<-atoms,do: atom)++(for i<-[-1000,-1,0,1,2,256,1_000_000,576460752303423487,-576460752303423488],do: i)++(for a<-atoms,b<-atoms,i<-[0,1,3,255],do: {a,b,i})++(for i<-0..300,do: [:'$e'|i])++[[],{},{:a,{:b,:c,2}},{{:m,:f,2},{:n,:g,1}},{:in,{:m,:f,2}},{:out,123},{[:a|2],{:m,:null,3}},[1,2,3,4,5]]
for {term,index}<-Enum.with_index(rows) do
 IO.puts(:json.encode(%{id: index,profile: profile,key: encode.(encode,term),hash: Integer.to_string(:erts_debug.get_internal_state({:internal_hash,term}))}))
end
:erts_debug.set_internal_state(:available_internal_state,previous)
