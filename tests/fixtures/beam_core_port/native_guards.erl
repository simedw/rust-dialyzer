-module(native_guards).
-export([choice/2, strict/2, inverse/1]).
choice(X,Y) when X; Y -> yes;
choice(_,_) -> no.
strict(X,Y) when X or Y -> yes;
strict(_,_) -> no.
inverse(X) when not X -> yes;
inverse(_) -> no.
