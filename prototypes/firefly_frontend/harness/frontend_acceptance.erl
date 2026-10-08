-module(frontend_acceptance).
-export([id/1, add/2, good/0, bad/0, wrong_return/0]).
-export_type([count/0]).
-type count() :: integer().
-spec id(count()) -> count().
id(X) -> X.
-spec add(integer(), integer()) -> integer().
add(A, B) when is_integer(A), is_integer(B) -> A + B.
-spec good() -> integer().
good() -> add(id(2), 3).
bad() -> add(oops, 3).
-spec wrong_return() -> integer().
wrong_return() -> oops.
