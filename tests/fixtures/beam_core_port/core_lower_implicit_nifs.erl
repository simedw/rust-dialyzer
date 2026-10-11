-module(core_lower_implicit_nifs).
-export([init/0,native/1]).
init() -> erlang:load_nif("file",0).
native(X) -> X.
