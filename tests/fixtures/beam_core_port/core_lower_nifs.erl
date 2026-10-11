-module(core_lower_nifs).
-export([init/0,native/1,ordinary/0]).
-nifs([native/1]).
init() -> erlang:load_nif("file",0).
native(X) -> X.
ordinary() -> ok.
