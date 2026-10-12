-module(core_lower_repeated_guards).
-export([multi/1, explicit/1, structural/1, andalso_guard/2]).
multi({X,X,Y,Y}) -> {X,Y}.
explicit({X,X}) when is_integer(X) -> X.
structural(F) -> {A,A,B,B} = F(), {A,B}.
andalso_guard(X,Y) when X andalso (is_integer(Y) andalso Y>0) -> yes;
andalso_guard(_,_) -> no.
