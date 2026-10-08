# Check-mode oracle (what dialyxir runs): warnings in harness wire format.
[plt, beams, out] = System.argv()
files = beams |> File.read!() |> String.split("\n", trim: true) |> Enum.map(&String.to_charlist/1)
w = :dialyzer.run(files: files, init_plt: String.to_charlist(plt), check_plt: false, warnings: [:unknown], error_location: :column, indent_opt: false)
defmodule Wire do
  def term(v) when is_atom(v), do: %{kind: "atom", value: Atom.to_string(v)}
  def term(v) when is_integer(v), do: %{kind: "integer", value: Integer.to_string(v)}
  def term(v) when is_float(v), do: %{kind: "float", value: Base.encode16(<<v::float-64>>)}
  def term(v) when is_binary(v), do: %{kind: "binary", value: Base.encode64(v)}
  def term(v) when is_tuple(v), do: %{kind: "tuple", items: Enum.map(Tuple.to_list(v), &term/1)}
  def term([]), do: %{kind: "list", items: [], tail: :null}
  def term([h | t]), do: list([h | t], [])
  defp list([h | t], acc), do: list(t, [term(h) | acc])
  defp list(t, acc), do: %{kind: "list", items: Enum.reverse(acc), tail: if(t == [], do: :null, else: term(t))}
end
File.write!(out, :json.encode(%{warnings: Wire.term(w), warning_count: length(w)}))
IO.puts(length(w))
