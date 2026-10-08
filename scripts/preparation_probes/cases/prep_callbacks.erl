-module(prep_callbacks).
-export([f/1]).
-type module() :: integer().
-export_type([module/0]).
-callback cb(module()) -> module().
-spec f(module()) -> module().
f(A) -> A.
