-module(native_impossible_aliases).
-export([literal_head/1, tuple_head/1, map_head/1, case_alias/1, bind_alias/1, bind_literal/0]).
literal_head(42 = wrong) -> impossible;
literal_head(X) -> X.
tuple_head({X} = {Y,Z}) -> {X,Y,Z};
tuple_head(X) -> X.
map_head(#{a := X,a := X = 3,a := 4}) -> X;
map_head(X) -> X.
case_alias(X) -> case X of {Y} = {Z,W} -> {Y,Z,W}; _ -> ok end.
bind_alias(X) -> ({Y} = {Z,W}) = X, {Y,Z,W}.
bind_literal() -> catch (#{a:=3,a:=4} = #{a=>3}).
