-module(whole_module_map_exact).
-export([entry/1]).
entry(X) -> M = #{a => X}, M#{missing := 2}.
helper(X) -> X.
unused(X) -> X.
