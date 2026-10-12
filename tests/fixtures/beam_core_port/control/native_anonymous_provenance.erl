-module(native_anonymous_provenance).
-export([unsafe/1,ignored/1,handler/1,bind/1,case_input/1,tryfun/1]).
unsafe(A) -> (fun(X) -> X+A end)(1).
ignored(A) -> _=lists:foreach(fun(X) -> X+A end,[1]), ok.
handler(A) -> try A+1 catch _:X -> (fun(Y) -> Y end)(X) end.
bind(A) -> F=fun(X) -> A+X end, F(1).
case_input(A) -> case (fun() -> A end)() of X -> X end.
tryfun(A) -> try (fun() -> A end)() of X -> X catch _:_ -> error end.
