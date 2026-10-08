# Normal OTP Dialyzer only: no Core conversion, export, or application recompilation.
[configuration] = System.argv()
config = configuration |> File.read!() |> :json.decode()
options = Enum.map(config["warning_options"], &String.to_atom/1)
started = System.monotonic_time(:nanosecond)
warnings = :dialyzer.run(files: Enum.map(config["beams"], &String.to_charlist/1),
  init_plt: String.to_charlist(config["plt"]), check_plt: false,
  warnings: [:no_unknown | options], error_location: :column, indent_opt: false)
elapsed = System.monotonic_time(:nanosecond) - started
# Serialization is deliberately outside the Dialyzer analysis timer.
defmodule BenchmarkWarnings do
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
IO.puts(:json.encode(%{dialyzer_ns: elapsed, warnings: BenchmarkWarnings.term(warnings),
  warning_count: length(warnings), profile: %{otp: List.to_string(:erlang.system_info(:otp_release)),
  erts: List.to_string(:erlang.system_info(:version)), elixir: System.version()}}))
