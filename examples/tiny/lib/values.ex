defmodule Tiny.Values do
  @spec ready() :: :ready
  def ready, do: :ready

  @spec greet() :: binary()
  def greet, do: "hello"

  @spec pair(integer(), atom()) :: {integer(), atom()}
  def pair(number, tag), do: {number, tag}

  @spec numbers() :: [integer()]
  def numbers, do: [1, 2, 3]

  @spec list_size(list()) :: non_neg_integer()
  def list_size(values), do: length(values)

  @spec first([integer(), ...]) :: integer()
  def first([head | _tail]), do: head

  @spec classify(integer()) :: :negative | :positive
  def classify(value) when value < 0, do: :negative
  def classify(_value), do: :positive

  @spec identity(atom()) :: atom()
  def identity(value), do: value

  @spec bytes(binary()) :: non_neg_integer()
  def bytes(value), do: byte_size(value)
  require Tiny.Macros
  @spec macro_double(integer()) :: integer()
  def macro_double(value), do: Tiny.Macros.twice(value)
end

