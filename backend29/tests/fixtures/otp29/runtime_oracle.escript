#!/usr/bin/env escript
%%! +S 2:2
main([Out]) ->
    "29" = erlang:system_info(otp_release),
    Old = erts_debug:set_internal_state(available_internal_state, true),
    %% Allocate fields in a different order from their names and declarations.
    Names = [list_to_atom("runtime_field_" ++ integer_to_list(I)) || I <- lists:seq(40, 0, -1)],
    Mk = fun(Mod, Name, Export, Fields) -> records:create(Mod, Name, Fields, #{is_exported => Export}) end,
    Small = Mk(runtime_module, small, true, [{lists:nth(3, Names), 1}, {lists:nth(1, Names), 2}, {lists:nth(2, Names), 3}]),
    Empty = Mk(runtime_module, empty, false, []),
    Large = Mk(runtime_module, large, true, lists:zip(Names, lists:seq(0,40))),
    Nested = Mk(runtime_module, nested, false, [{payload, #{Small => Large}}, {item, Empty}]),
    Ordered = Mk(runtime_module, reordered, false, [{first,1},{second,2}]),
    Reversed = Mk(runtime_module, reordered, false, [{second,2},{first,1}]),
    Terms = [Empty, Small, Large, Nested, {Small, Large}, [Small, Empty],
             #{Small => Large, Empty => Nested},
             maps:from_list([{I, {Small,I}} || I <- lists:seq(1,40)]),
             #{a => 1, b => 2}, #{#{a => 1, b => 2} => nested_map},
             maps:from_list([{list_to_atom("ordinary_" ++ integer_to_list(I)), I} || I <- lists:seq(1,40)]),
             Ordered,Reversed,#{Ordered => 1,Reversed => 2},
             Mk(runtime_module,reordered,true,[{first,1},{second,2}]),
             Mk(runtime_module,reordered,false,[{first,2},{second,1}]),
             Mk(runtime_module,numeric,false,[{value,1}]),
             Mk(runtime_module,numeric,false,[{value,1.0}])],
    Atoms = lists:usort(lists:append([atoms(T) || T <- Terms]) ++ [true,false]),
    Layout = #{version => 1,word_size => 8,scope => <<"otp29-runtime-oracle">>,
        atom_hashes => maps:from_list([{atom_to_binary(A),integer_to_binary(erts_debug:get_internal_state({internal_hash,A}))} || A <- Atoms])},
    Rows = [#{etf => binary_to_list(term_to_binary(T)),
              internal_hash => integer_to_binary(erts_debug:get_internal_state({internal_hash,T})),
              phash => erlang:phash(T, 16#100000000), phash2 => erlang:phash2(T,16#100000000)} || T <- Terms],
    Comparisons = [[compare(A,B),compare_exact(A,B)] || A <- Terms, B <- Terms],
    ok = file:write_file(Out,json:encode(#{otp => list_to_binary(erlang:system_info(otp_release)),
        erts => list_to_binary(erlang:system_info(version)),layout => Layout,rows => Rows,comparisons => Comparisons})),
    erts_debug:set_internal_state(available_internal_state,Old).
atoms(T) when is_record(T) -> [records:get_module(T),records:get_name(T)] ++ records:get_field_names(T) ++ lists:append([atoms(records:get(K,T)) || K <- records:get_field_names(T)]);
atoms(T) when is_atom(T) -> [T];
atoms(T) when is_tuple(T) -> lists:append([atoms(X) || X <- tuple_to_list(T)]);
atoms(T) when is_map(T) -> lists:append([atoms(K) ++ atoms(V) || {K,V} <- maps:to_list(T)]);
atoms([H|T]) -> atoms(H) ++ atoms(T);
atoms(_) -> [].
compare(A,B) when A == B -> <<"equal">>;
compare(A,B) when A < B -> <<"less">>;
compare(_,_) -> <<"greater">>.
compare_exact(A,B) -> case erts_internal:cmp_term(A,B) of N when N < 0 -> <<"less">>; 0 -> <<"equal">>; _ -> <<"greater">> end.
