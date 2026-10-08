Code.require_file("../compiler_atom_primitives.exs",__DIR__)
:logger.set_primary_config(:level,:none)
atoms=[:m,:f,:g,:in,:out]++Enum.map(0..1499,&String.to_atom("module_#{&1}"))
profile=CompilerAtomPrimitives.export(atoms,"map-order-proof-1")
encode=fn encode,term->cond do
 is_atom(term)->%{atom: Atom.to_string(term)}
 is_integer(term)->%{integer: Integer.to_string(term)}
 is_tuple(term)->%{tuple: term|>Tuple.to_list()|>Enum.map(&encode.(encode,&1))}
 term==[]->%{nil: true}
 true->[head|tail]=term;%{cons: [encode.(encode,head),encode.(encode,tail)]}
end end
for shape<-0..5,n<-[0,1,2,10,31,32,33,34,64,100,500,1000,1500],reverse<-[false,true] do
 keys=if n==0,do: [],else: Enum.map(0..(n-1),fn i->case shape do
 0->i
 1->{:m,:f,i}
 2->{String.to_atom("module_#{i}"),:g,rem(i,3)}
 3->String.to_atom("module_#{i}")
 4->{String.to_atom("module_#{i}"),String.to_atom("module_#{rem(i*17,1500)}")}
 5->{rem(i,2),String.to_atom("module_#{i}"),{i,:m}}
 end end)
 keys=if reverse,do: Enum.reverse(keys),else: keys
 map=Map.new(keys,&{&1,true})
 IO.puts(:json.encode(%{id: [shape,n,reverse],profile: profile,insert: Enum.map(keys,&encode.(encode,&1)),keys: Enum.map(:maps.to_list(map),fn {k,_}->encode.(encode,k) end)}))
end
