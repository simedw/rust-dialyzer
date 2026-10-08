-module(prep_nominal_shadow).
-export([f/1]).
-type module() :: integer().
-nominal wrapped() :: module().
-record(r, {a :: wrapped(), b :: module()}).
-export_type([wrapped/0]).
-spec f(#r{}) -> #r{}.
f(A) -> A.
