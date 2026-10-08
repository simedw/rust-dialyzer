defmodule Tiny.Macros do
  defmacro twice(value) do
    quote do: unquote(value) + unquote(value)
  end
end

