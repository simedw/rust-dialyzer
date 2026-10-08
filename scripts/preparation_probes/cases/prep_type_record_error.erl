-module(prep_type_record_error).
-export([f/1]).
-record(r, {a :: integer()}).
-type bad() :: #r{a :: atom()}.
-export_type([bad/0]).
f(A) -> A.
