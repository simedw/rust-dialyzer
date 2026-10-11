-module(native_record_guards).
-export([plain/1, qualified/1, conjunction/2, alternatives/1, negated/1,
         function_arity/2, function_arity_and/2, pinned/2, repeated/2,
         two_pins/3, protection/2]).
-record(r,{x=none}).
plain(R) when is_record(R,r) -> R;
plain(_) -> other.
qualified(R) when erlang:is_record(R,r,2) -> R;
qualified(_) -> other.
conjunction(R,N) when is_record(R,r), N>0 -> R;
conjunction(_,_) -> other.
alternatives(R) when is_record(R,r); is_list(R) -> R;
alternatives(_) -> other.
negated(R) when not is_record(R,r) -> R;
negated(_) -> other.
function_arity(F,A) when is_function(F,A) -> yes;
function_arity(_,_) -> no.
function_arity_and(F,A) when is_function(F,A), is_integer(A) -> yes;
function_arity_and(_,_) -> no.
pinned(Ref,Msg) ->
    case Msg of
        {Ref,Verb} when Verb =:= join; Verb =:= leave -> match;
        _ -> no_match
    end.
repeated({X,X},Flag) when Flag =:= a; Flag =:= b -> X;
repeated(_,_) -> other.
two_pins(A,B,{A,B,V}) when V == a; V == b -> V;
two_pins(_,_,_) -> other.
protection(Ref,{Ref,V}) when is_integer(length(V)), V =/= []; is_atom(V) -> V;
protection(_,_) -> other.
