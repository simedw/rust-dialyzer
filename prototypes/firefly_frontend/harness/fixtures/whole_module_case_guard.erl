-module(whole_module_case_guard).
-export([entry/1]).
entry(X) -> case X of {ok, V} when is_integer(V) -> V + 1; [] -> 0; _ -> error(bad) end.
helper(X) -> X.
unused(X) -> X.
