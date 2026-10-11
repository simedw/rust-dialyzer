-module(native_aliases).
-compile(export_all).
-record(r,{x}).
bin_head(<<A:8,B:8>> = <<C:16>>) -> {A,B,C}.
bin_empty(<<_:0>> = <<>>) -> zero.
bin_dep(<<N:8,B:N/binary>> = <<C:16>>) -> {N,B,C}.
bin_multi(X) -> case X of <<N:8,B:N/binary>> = <<M:16,C:M/binary>> -> {N,B,M,C}; _ -> no end.
map_dup(#{a:=X,a:=Y}) -> {X,Y}.
map_alias(#{a:=X}=#{a:=Y}) -> {X,Y}.
list_alias([X|T]="abc") -> {X,T}.
list_alias_reverse("abc"=[X|T]) -> {X,T}.
receive_alias() -> receive <<N:8,B:N/binary>> = <<C:16>> -> {N,B,C} after 0 -> no end.
bin_repeat(<<N:8,N:8,B:N/binary>>) -> {N,B}.
bin_pin(<<N:8,B:N/binary>>,N) -> B.
map_key_expr(X,T) -> case X of #{(element(1,T)):=V} -> V end.
nested_record(E) -> St = #r{x=X} = E,{St,X}.
nested_structural(E) -> A = {X,Y} = E,{A,X,Y}.
nested_reason(X) -> try erlang:error(X) catch error:Reason -> try erlang:error(foo) catch error:Reason -> same; error:Other -> {different,Other} end end.
record_guard(X) when is_record(X,r) -> yes; record_guard(_) -> no.
pinned_guard(X) -> case X of X when X > 0, X < 10; X =:= 99 -> yes; _ -> no end.
