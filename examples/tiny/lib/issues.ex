defmodule Tiny.Issues do
  def bad_builtin, do: byte_size(42)
  def bad_remote, do: Tiny.Math.increment(:wrong)
  def propagated, do: bad_remote()
end

