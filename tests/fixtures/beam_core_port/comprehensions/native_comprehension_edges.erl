-module(native_comprehension_edges).
-export([shadow/2,repeat/1,strictguard/1,strictbin/1,strictmap/1,zipmixed/2,zipbins/2,append/2,constbin/1,filters/1]).
shadow(L,X) -> [X || X <- L].
repeat(L) -> [X || {X,X} <- L].
strictguard(L) -> [X || X <:- L, false].
strictbin(B) -> [X || <<X:8>> <:= B].
strictmap(M) -> [V || _ := {ok,V} <:- M].
zipmixed(A,B) -> [{X,Y} || X <- A && {ok,Y} <:- B].
zipbins(A,B) -> [{X,Y} || <<X>> <= A && <<Y>> <:= B].
append(L,T) -> [X || X <- L] ++ T.
constbin(L) -> << <<1:4>> || _ <- L >>.
filters(L) -> [X || X <- L, is_integer(X), (X > 0) andalso (X < 10), foo(X), X =:= 1].
foo(X) -> X =:= 1.
