-module(native_control_more).
-export([pin/2,else_pin/2,repeat/1,matchlast/2,none_else/1,nested/1,parameter/0,self_match/0,named_rhs/1,ordinary_before_match/1,named_unsafe/1]).
pin(X,A) -> maybe X ?= A, ok else _ -> fail end.
else_pin(X,A) -> maybe {ok,Y} ?= A, Y else X -> same; _ -> other end.
repeat(A) -> maybe {X,X} ?= A, X end.
matchlast(A,B) -> maybe {ok,X} ?= A, {ok,X} ?= B end.
none_else(A) -> maybe A+1 else _ -> fail end.
nested(A) -> maybe {ok,X} ?= A, maybe {ok,Y} ?= X, Y end end.
parameter() -> fun F(F) -> F end.
self_match() -> fun F(X) -> F=X, F end.

named_rhs(A) -> F=fun G(0) -> 0; G(X) -> G(X-1) end, F(A).
ordinary_before_match(A) -> maybe {ok,X}=get(A), true ?= is_integer(X), X end.

named_unsafe(A) -> try (fun F(0) -> A; F(N) -> F(N-1) end)(3) catch _:_ -> error end.
