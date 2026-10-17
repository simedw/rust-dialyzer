-module(comprehension_flow29).
-export([multi/1, nested/1, map_multi/1, from_keys/2]).
multi(Xs) -> [X, X + 1 || X <- Xs, is_integer(X)].
nested(Xs) -> [Y || X <- Xs, Y <- [X + 1]].
map_multi(Xs) -> #{X => X, X + 1 => X || X <- Xs, is_integer(X)}.
from_keys(Xs, Value) -> maps:from_list([{X, Value} || X <- Xs]).
