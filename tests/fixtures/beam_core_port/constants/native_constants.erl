-module(native_constants).
-export([negative/1, big/1, multiply/1, division/1, remainder/1, bits/1,
         shift_right/1, shift_left/1, negative_shift/1, float_neg/1, float_zero/1,
         float_add/1, float_mul/1, float_div/1, comparison/1, exact_comparison/1,
         bool_not/1, bool_and/1, bool_or/1, bool_xor/1, append/1, append_cons/1,
         map_numeric/1, map_float/1]).
negative(-42) -> negative.
big(-(1 bsl 90)) -> big.
multiply(17 + 13 * 41) -> multiply.
division(-17 div 3) -> division.
remainder(-17 rem 3) -> remainder.
bits((bnot 17) band 255 bor (2 bxor 7)) -> bits.
shift_right(-42 bsr 3) -> right.
shift_left(13 bsl 130) -> left.
negative_shift(16 bsl -3) -> negative_shift.
float_neg(-2.5) -> float_neg.
float_zero(-0.0) -> negative_zero.
float_add(1.25 + 4) -> float_add.
float_mul(-1.25 * 2.0) -> float_mul.
float_div(1 / 4) -> float_div.
comparison(#{(1 == 1.0) := V}) -> equal.
exact_comparison(#{(1 =:= 1.0) := V}) -> exact.
bool_not(#{(not false) := V}) -> bool_not.
bool_and(#{(true and false) := V}) -> bool_and.
bool_or(#{(true or false) := V}) -> bool_or.
bool_xor(#{(true xor true) := V}) -> bool_xor.
append("ab" ++ Tail) -> Tail.
append_cons([$a,$b] ++ Tail) -> Tail.
map_numeric(#{(1 bsl 70) := V}) -> V.
map_float(#{(-0.0) := V}) -> V.
