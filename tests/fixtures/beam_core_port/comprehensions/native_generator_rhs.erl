-module(native_generator_rhs).
-export([head/0,second/0]).
head() -> [fun(Z) -> {X,Z} end || X <- (Z=[1,2])].
second() -> [Y || X <- (Z=[1,2]), Y <- (fun(Z) -> Z end)(X)].
