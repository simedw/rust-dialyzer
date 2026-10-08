-module(prep_opaque_shadow).
-export([f/1]).
-type module() :: integer().
-opaque wrapped() :: module().
-record(r, {a :: wrapped(), b :: module()}).
-export_type([wrapped/0]).
-spec f(#r{}) -> #r{}.
f(A) -> A.
