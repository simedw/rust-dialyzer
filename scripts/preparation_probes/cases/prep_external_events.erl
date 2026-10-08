-module(prep_external_events).
-export([f/1]).
-opaque hidden() :: prep_missing_lib:thing().
-type local() :: prep_other_missing:thing().
-record(r, {a :: prep_missing_lib:thing(), b :: local(), c :: hidden()}).
-export_type([hidden/0, local/0]).
-spec f(#r{}) -> prep_missing_lib:thing().
-callback cb(prep_missing_lib:thing()) -> prep_missing_lib:thing().
f(A) -> A.
