-module(whole_module_try).
-export([entry/1]).
entry(X) -> try helper(X) of {ok,V} -> V catch error:bad -> recovered end.
helper(X) -> case X of {ok,_} -> X; _ -> error(bad) end.
unused(X) -> X.
