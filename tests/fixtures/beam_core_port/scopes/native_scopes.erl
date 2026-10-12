-module(native_scopes).
-compile(export_all).
left(A) -> {X=2,fun() -> X=99,A=1 end()}.
right(A) -> {fun() -> X=99,A=1 end(),X=2}.
both() -> {X=1,X=1},X.
call(A) -> erlang:append_element(fun() -> X=99,A=1 end(),X=2),X.
