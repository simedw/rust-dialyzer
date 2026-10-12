-module(native_matches).
-compile(export_all).
arg_match(X) -> erlang:abs(Y=X),Y.
arg_literal() -> {X=1},X.
nested(X) -> A=B=X,{A,B}.
nested_struct(X) -> {A,B}=All=X,{A,B,All}.
match_case(X) -> case {Y}=X of _ -> Y end.
call_struct(X) -> erlang:is_tuple({A,B}=X),{A,B}.
pin_arg(X) -> erlang:abs(X=1).
nested_literal() -> X=Y={a,b},{X,Y}.
head_alias({X,Y}={a,B}) -> {X,Y,B}.
head_map_alias(#{a:=X}=#{a:=Y,b:=B}) -> {X,Y,B}.
bin(X) -> case X of <<N:8,B:N/binary,T/binary>> -> {N,B,T}; _ -> no end.
bin_expr(X,N) -> case X of <<B:(N+1)/binary,T/binary>> -> {B,T}; _ -> no end.
bin_many(X) -> case X of <<N:8,B:N/binary,M:8,C:M/binary>> -> {B,C}; _ -> no end.
bin_tuple(X) -> case X of {tag,<<N:8,B:N/binary>>} -> B; _ -> no end.
bin_alias(X) -> case X of All = <<N:8,B:N/binary>> -> {All,B}; _ -> no end.
map_expr(X,N) -> case X of #{(N+1):=V} -> V; _ -> no end.
map_tuple(X,N) -> case X of #{ {N,a}:=V } -> V; _ -> no end.
map_bin(X) -> case X of #{a:=<<N:8,B:N/binary>>} -> B; _ -> no end.
receive_bin() -> receive <<N:8,B:N/binary>> -> B after 0 -> none end.
block(X) -> Y=begin case X of a -> V=1; _ -> V=2 end,V end,{Y,V}.
