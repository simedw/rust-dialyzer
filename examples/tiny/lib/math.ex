defmodule Tiny.Math do
  @spec add(integer(), integer()) :: integer()
  def add(a, b), do: a + b

  @spec double(integer()) :: integer()
  def double(value), do: add(value, value)

  @spec half(number()) :: float()
  def half(value), do: value / 2

  @spec absolute(integer()) :: non_neg_integer()
  def absolute(value) when value < 0, do: -value
  def absolute(value), do: value

  def increment(value), do: value + 1

  @spec wrong_return(integer()) :: binary()
  def wrong_return(value), do: value + 1
  @spec count_down(integer()) :: integer()
  def count_down(0), do: 0
  def count_down(value) when is_integer(value), do: count_down(value - 1)

  def even(0), do: true
  def even(value) when is_integer(value), do: odd(value - 1)
  def odd(0), do: false
  def odd(value) when is_integer(value), do: even(value - 1)
end

