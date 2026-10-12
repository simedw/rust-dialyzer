defmodule NativeRegexBlob do
  def compiled, do: ~r/^[a-z][a-z0-9_]*(\.[A-Z]\w*)*$/iu
  def matches?(name), do: name =~ ~r/^[a-z][a-z0-9_]*$/
end
