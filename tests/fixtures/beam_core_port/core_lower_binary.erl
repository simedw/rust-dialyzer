-module(core_lower_binary).
-export([constant/0, dynamic/3, pattern/1, bits/2, utf/1, string_dynamic/1]).
constant() -> <<17:4, -1:12/little, 1.5:64/float, 955/utf8>>.
dynamic(X, B, N) -> <<X:16/little-signed, B:N/binary, X/utf16-little>>.
pattern(<<A:8,B:16/little,Rest/binary>>) -> {A,B,Rest}.
bits(X,N) -> <<X:N/unit:2, X:3>>.
utf(C) -> <<C/utf8,C/utf32-little>>.
string_dynamic(X) -> <<"prefix",X:8,"tail">>.
