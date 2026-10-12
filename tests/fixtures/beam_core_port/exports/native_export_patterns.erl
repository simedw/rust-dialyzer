-module(native_export_patterns).
-export([map_key/2,binary_size/2,captured_key/2]).
map_key(A,B) -> case A of a -> X=foo; b -> X=bar end, case B of #{X := Y} -> Y end.
binary_size(A,B) -> case A of a -> N=8; b -> N=16 end, case B of <<X:N>> -> X end.
captured_key(A,B) -> case A of a -> X=foo; b -> X=bar end, F=fun(#{X := Y}) -> Y end, F(B).
