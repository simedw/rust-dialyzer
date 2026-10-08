-module(firefly_rethrow_identity).
-export([catch_rethrow/1, after_rethrow/1, explicit_raise/0]).
catch_rethrow(X) -> try X() catch error:handled -> caught end.
after_rethrow(X) -> try X() after done end.
explicit_raise() -> erlang:raise(error, reason, []).
