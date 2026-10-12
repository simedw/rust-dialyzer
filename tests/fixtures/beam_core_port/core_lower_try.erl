-module(core_lower_try).
-export([plain/1, stack/1, of_clause/1, rethrow/1, catching/1]).
plain(F) -> try F() catch error:badarg -> failed; throw:E -> {thrown,E} end.
stack(F) -> try F() catch Class:Reason:Stack -> {Class,Reason,Stack} end.
of_clause(F) -> try F() of {ok,V} -> V; error -> none catch error:badarg -> failed end.
rethrow(F) -> try F() of X -> {wrapped,X} catch throw:ignore -> ignore end.
catching(F) -> catch F().
