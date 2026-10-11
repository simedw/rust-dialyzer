defmodule NativeSlice do
  @moduledoc false
  @spec identity(term()) :: term()
  def identity(x), do: x
  def value(), do: :ready
  def pair(x), do: {x, helper(x)}
  def remote(x), do: :erlang.is_atom(x)
  defp helper(x), do: x
end
