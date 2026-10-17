-module(native_comprehensions29).
-feature(compr_assign, enable).
-export([list/1, assignment/1, map_keys/2, map_values/1, map_multiple/2,
         filter_binding/1, nested/1, map_mixed/2, map_tuple/2, append/2,
         assignment_nested/1]).
list(Xs) -> [X, X+1 || X <- Xs].
assignment(Xs) -> [Y, Y+1 || X <- Xs, Y = X*2].
map_keys(Xs, V) -> #{K => V || K <- Xs}.
map_values(Xs) -> #{K => K*2 || K <- Xs}.
map_multiple(Xs, V) -> #{K => V, K+1 => V || K <- Xs}.
filter_binding(Xs) -> #{K => V || K <- Xs, is_integer(V = K*2)}.
nested(Xs) -> #{K => [X, X+1 || X <- K] || K <- Xs}.
map_mixed(Xs, V) -> #{K => V, K+1 => K || K <- Xs}.
map_tuple(Xs, V) -> #{K => {V,1} || K <- Xs}.
append(Xs, Tail) -> [X, X+1 || X <- Xs] ++ Tail.
assignment_nested(Xs) -> [Y, Z || X <- Xs, Y = Z = X*2].
