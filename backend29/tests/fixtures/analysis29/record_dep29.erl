-module(record_dep29).
-export_record([point]).
-record #point{x = 0 :: integer(), y = 0 :: integer()}.
-export([make/1]).
make(X) -> #point{x = X}.
