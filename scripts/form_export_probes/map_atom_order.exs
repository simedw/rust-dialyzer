[order]=System.argv()
names=["dialyzer_context_order_a_new","dialyzer_context_order_b_new"]
allocation=if order=="ba",do: Enum.reverse(names),else: names
allocated=Enum.map(allocation,&String.to_atom/1)
[a,b]=Enum.map(names,&String.to_existing_atom/1)
m=Map.new([{a,1},{b,2}])
to_list=:maps.to_list(m);iter=:maps.to_list(:maps.iterator(m));ordered=:maps.to_list(:maps.iterator(m,:ordered));physical=:erts_internal.map_to_tuple_keys(m)
IO.puts(:json.encode(%{allocation: Enum.map(allocated,&Atom.to_string/1),to_list: Enum.map(to_list,fn {k,_}->Atom.to_string(k)end),iterator: Enum.map(iter,fn {k,_}->Atom.to_string(k)end),ordered: Enum.map(ordered,fn {k,_}->Atom.to_string(k)end),physical: physical|>Tuple.to_list()|>Enum.map(&Atom.to_string/1),otp: List.to_string(:erlang.system_info(:otp_release)),erts: List.to_string(:erlang.system_info(:version))}))
