-module(native_comprehension_more).
-export([none/0,repeatguard/1,repeatboth/1,repeatbin/1,complexbin/1,zipmap/2,zipstrict/2,tuplepattern/1,nestedmc/1,constbinary/1]).
none() -> [1 || true].
repeatguard(L) -> [X || {X,X} <- L, is_integer(X)].
repeatboth(L) -> [{X,Y} || {X,X,Y,Y} <- L, is_atom(X), is_integer(Y)].
repeatbin(B) -> [X || <<X,X>> <= B].
complexbin(B) -> [X || <<0:1,X:7>> <= B].
zipmap(A,B) -> [{K,V,X} || K := V <- A && {ok,X} <:- B].
zipstrict(A,B) -> [{X,Y} || {ok,X} <:- A && {ok,Y} <:- B].
tuplepattern(L) -> [X || #{x:=X} <- L].
nestedmc(L) -> #{X => Y || X <- L, Y <- X}.
constbinary(L) -> << (constant()) || _ <- L >>.
constant() -> <<1,2>>.
