-module(native_named_pattern).
-export([map_key/0,bin_size/0,nested_map_key/0]).
map_key() -> fun F(#{F := X}) -> X end.
bin_size() -> fun F(<<X:F>>) -> X end.
nested_map_key() -> fun F(X) -> fun(#{F := Y}) -> {X,Y} end end.
