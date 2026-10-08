Code.require_file("../compiler_atom_primitives.exs",__DIR__)
:logger.set_primary_config(:level,:none)
atoms=[:m,:f,:g,:in,:out,:"$e"]++Enum.map(0..1499,&String.to_atom("module_#{&1}"))
profile=CompilerAtomPrimitives.export(atoms,"ets-proof-1")
encode=fn encode,t->cond do is_atom(t)->%{atom: Atom.to_string(t)};is_integer(t)->%{integer: Integer.to_string(t)};is_tuple(t)->%{tuple: t|>Tuple.to_list()|>Enum.map(&encode.(encode,&1))};t==[]->%{nil: true};true->[h|tail]=t;%{cons: [encode.(encode,h),encode.(encode,tail)]}end end
for shape<-0..3,n<-[0,1,2,10,50,127,128,255,256,257,300,511,512,513,1000,2048,2304,2305,3000,8000],reverse<-[false,true],remove<-[0,1,20,100,500,2000,7500] do
 keys=for i<-Enum.take(Stream.iterate(0,&(&1+1)),n) do
  case shape do 0->i;1->{:m,:f,i};2->{Enum.at(atoms,6+rem(i,1500)),:g,rem(i,3)};3->[:"$e"|i] end
 end
 keys=if reverse,do: Enum.reverse(keys),else: keys
 table=:ets.new(:fixture,[:set,:protected])
 Enum.each(keys,&:ets.insert(table,{&1,1}))
 removed=Enum.take(keys,remove)
 Enum.each(removed,&:ets.delete(table,&1))
 selected=:ets.select(table,[{{:"$1",:_},[],[:"$1"]}])
 :ets.delete(table)
 IO.puts(:json.encode(%{id: [shape,n,reverse,remove],profile: profile,insert: Enum.map(keys,&encode.(encode,&1)),remove: Enum.map(removed,&encode.(encode,&1)),keys: Enum.map(selected,&encode.(encode,&1))}))
end
