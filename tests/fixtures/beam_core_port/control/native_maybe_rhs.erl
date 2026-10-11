-module(native_maybe_rhs).
-export([nested_match/1,block/1]).
nested_match(A) -> maybe {ok,X} ?= {ok,Y=A}, {X,Y} end.
block(A) -> maybe {ok,X} ?= begin Y=A, {ok,Y} end, X end.
