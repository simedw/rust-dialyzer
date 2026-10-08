-module(engine_structural).
-export([unsigned/1,signed/1,remainder/1,utf/1,make/1,read/1,put/1,try_pure/0,catch_pure/0,capture/0,dynamic/3,dynamic_known/0,bad_dynamic/0,guard_tuple/1]).
unsigned(<<X:8>>) -> X.
signed(<<X:8/signed>>) -> X.
remainder(<<_:8, X/binary>>) -> X.
utf(<<X/utf8>>) -> X.
make(X) -> <<X:8, 42:8>>.
read(#{a:=X}) -> X.
put(M) -> M#{a=>42}.
try_pure() -> try ok of X -> X catch _:_ -> caught end.
catch_pure() -> catch ok.
capture() -> X=42, F=fun() -> X end, F().

dynamic(M,F,X) -> M:F(X).
dynamic_known() -> M=engine_structural, M:capture().
bad_dynamic() -> M=42, M:capture().

guard_tuple(X) when is_record({X,[]},tag,2) -> X.
