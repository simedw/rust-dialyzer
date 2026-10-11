-module(native_literal_shadow).
-export([captured/1, uncaptured/0, shadowed/1, ordinary/0, case_pattern/1]).
captured(X) ->
    fun({a,b}) -> X;
       ({Z,[c,d]}) -> {X,Z};
       ({N,#{key := {e,f}}}) -> {X,N};
       ([H,{a,b},[short]]) -> {X,H};
       ([literal | improper]) -> X
    end.
uncaptured() ->
    fun({a,b}) -> constant;
       ([c,d]) -> another
    end.
shadowed(X) ->
    fun({X,[b,c]}) -> X end.
ordinary() -> {a,[b,c],{d,e}}.
case_pattern(X) ->
    case X of {a,b} -> tuple; [c,d] -> list; _ -> other end.
