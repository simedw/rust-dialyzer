-module(whole_module_improper_cons).
-export([entry/1]).
entry(X) -> [X | 3].
helper(X) -> X.
unused(X) -> X.
