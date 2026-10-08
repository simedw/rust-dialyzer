-module(prep_bounded_record_error).
-export([f/1]).
-record(r, {a :: integer()}).
-spec f(A) -> #r{} when A :: #r{a :: atom()}.
f(A) -> A.
