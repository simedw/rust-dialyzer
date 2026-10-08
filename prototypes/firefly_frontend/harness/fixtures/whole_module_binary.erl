-module(whole_module_binary).
-export([entry/1]).
entry(X) -> <<X:8/integer>>.
helper(X) -> X.
unused(X) -> X.
