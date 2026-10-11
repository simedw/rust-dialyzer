-module(native_zip_patterns).
-export([shared/2,mixed/2,map/2]).
shared(L,M) -> [X || X <:- L && X <- M].
mixed(L,M) -> [{X,Y} || X <:- L && #{x := X,y := Y} <- M].
map(L,M) -> [X || X <:- L && {tag,#{x := X}} <- M].
