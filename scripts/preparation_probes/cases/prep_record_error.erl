-module(prep_record_error).
-export([f/1]).
-record(r, {a :: integer()}).
-spec f(#r{a :: atom()}) -> #r{}.
f(A) -> A.
