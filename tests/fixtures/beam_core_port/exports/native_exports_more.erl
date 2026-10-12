-module(native_exports_more).
-export([set_rebind/1,block/1,tail_nested/1,try_arg/1,try_inner/1,fun_capture/1,guard_rebind/1]).
set_rebind(A) -> Y=case A of a -> X=1; b -> X=2 end, X=1, {Y,X}.
block(A) -> Y=begin case A of a -> X=1; b -> X=2 end, X end, {Y,X}.
tail_nested(A) -> case A of a -> case A of a -> X=1; b -> X=2 end; b -> X=3 end, {X,A}.
try_arg(A) -> try X=case A of a -> Y=1; b -> Y=2 end of _ -> {X,Y} catch _:_ -> error end.
try_inner(A) -> try case A of a -> X=1; b -> X=2 end, X catch _:_ -> error end.
fun_capture(A) -> case A of a -> X=1; b -> X=2 end, fun() -> X end.
guard_rebind(A) -> case A of a -> X=1; b -> X=2 end, case A of X -> yes; _ -> no end.
