-module(native_strict_wildcards).
-export([strict_guard/1,strict_repeat/1]).
strict_guard(L) -> [DL || {_,{_,{From,To}}=DL} <:- L, From=/=To].
strict_repeat(L) -> [Y || {X,X,_}=Y <:- L, is_atom(X)].
