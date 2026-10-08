[input, out] = System.argv()
{:elixir_v1, metadata, _} = File.read!(input) |> :erlang.binary_to_term()
functions = for {na, :def, _, _} <- metadata.definitions, na not in metadata.unreachable, do: na
macros = for {na, :defmacro, _, _} <- metadata.definitions, na not in metadata.unreachable, do: na
term = {Enum.sort(functions), Enum.sort(macros), metadata.struct}
bytes = :erlang.term_to_binary(term, [:deterministic])
File.write!(out, bytes)
IO.inspect(%{input: term, md5: Base.encode16(:erlang.md5(bytes), case: :lower)})
