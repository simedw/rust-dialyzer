-module(native_comprehension).
-export([list/1, pattern/1, guard/1, filter/1, nested/1, binary/1, binlist/1, map/1, zip/2, strict/1]).
list(L) -> [X || X <- L].
pattern(L) -> [X || {ok,X} <- L].
guard(L) -> [X*2 || X <- L, X > 0].
filter(L) -> [X || X <- L, predicate(X)].
predicate(X) -> X =:= ok.
nested(L) -> [{X,Y} || X <- L, Y <- X].
binary(L) -> << <<X>> || X <- L >>.
binlist(B) -> [X || <<X>> <= B].
map(M) -> #{K=>V || K := V <- M}.
zip(A,B) -> [{X,Y} || X <- A && Y <- B].
strict(L) -> [X || {ok,X} <:- L].
