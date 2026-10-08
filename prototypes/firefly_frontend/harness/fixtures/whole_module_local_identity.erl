-module(whole_module_local_identity).
-export([entry/1]).
entry(X) -> helper(X).
helper(X) -> X.
unused(X) -> X.
