-module(core_lower_receive).
-export([plain/0,timeout/1,after_only/1,nested/0]).
plain() -> receive {tag,X} -> X; stop -> stopped end.
timeout(T) -> receive {tag,X} when is_integer(X) -> X after T -> timeout end.
after_only(T) -> receive after T -> timeout end.
nested() -> receive go -> receive done -> ok after 0 -> immediate end after 10 -> outer end.
