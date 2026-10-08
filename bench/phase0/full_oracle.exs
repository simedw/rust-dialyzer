# Derived from reports/real-projects/fixes/inferred-signatures/oracle.exs.
# Differences: `--unknown` keeps warn_unknown enabled (dialyxir's default), and
# warnings are also written as otp-warnings.json in the harness wire format
# (read by bench/phase0/compare_warnings.py).
#
#   elixir full_oracle.exs REPO CONFIG OUT_DIR [--public-only] [--unknown]
[root, config_path, out | options] = System.argv()
Code.require_file("type_wire.exs", Path.join(root, "scripts"))
config = config_path |> File.read!() |> :json.decode()
File.mkdir_p!(out)
output_plt = Path.join(out, "target-inference.plt")
if File.exists?(output_plt), do: raise("fresh output PLT required: #{output_plt}")
hash = fn p -> :crypto.hash(:sha256, File.read!(p)) |> Base.encode16(case: :lower) end
inputs = [config["plt"] | config["beams"]]
before = Map.new(inputs, fn p -> {p, hash.(p)} end)
modules = Enum.map(config["beams"], fn p ->
  {:ok, {module, _}} = :beam_lib.chunks(String.to_charlist(p), [:exports])
  module
end)
defmodule FullSignatureCapture do
  def capture(plt) do
    {modules, out} = :persistent_term.get({__MODULE__, :config})
    rows = :ets.tab2list(elem(plt, 1))
    entries = for {{module, function, arity}, {result, args}} <- rows,
        module in modules,
        do: %{module: Atom.to_string(module), function: Atom.to_string(function), arity: arity,
          result: NativeTypeWire.encode(result), arguments: Enum.map(args, &NativeTypeWire.encode/1)}
    File.write!(Path.join(out, "otp-full-signatures.json"), :json.encode(%{signatures: Enum.sort_by(entries, &{&1.module, &1.function, &1.arity}),
      anonymous_label_count: Enum.count(rows, fn {key, _} -> is_integer(key) end)}))
    :ok
  end
  def walk({:match, ann, {:var, _, :Plt4}, {:call, _, {:remote, _, {:atom, _, :dialyzer_plt}, {:atom, _, :delete_list}}, [{:var, _, :Plt3}, {:var, _, :NonExportsList}]}} = node) do
    call = {:call, ann, {:remote, ann, {:atom, ann, __MODULE__}, {:atom, ann, :capture}}, [{:var, ann, :Plt3}]}
    Process.put(:capture_sites, Process.get(:capture_sites, 0) + 1)
    {:block, ann, [call, node]}
  end
  def walk(term) when is_tuple(term), do: term |> Tuple.to_list() |> Enum.map(&walk/1) |> List.to_tuple()
  def walk(term) when is_list(term), do: Enum.map(term, &walk/1)
  def walk(term), do: term
end
if "--public-only" not in options do
:persistent_term.put({FullSignatureCapture, :config}, {modules, out})
module = :dialyzer_analysis_callgraph
original_path = :code.which(module)
{:ok, {^module, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(original_path, [:abstract_code])
forms = FullSignatureCapture.walk(forms)
1 = Process.get(:capture_sites)
{:ok, ^module, instrumented, _} = :compile.forms(forms, [:binary, :return_errors, :return_warnings])
{:module, ^module} = :code.load_binary(module, ~c"read-only full signature capture", instrumented)
File.write!(Path.join(out, "capture-provenance.json"), :json.encode(%{original_beam: List.to_string(original_path), original_sha256: hash.(List.to_string(original_path)),
  instrumented_sha256: :crypto.hash(:sha256, instrumented) |> Base.encode16(case: :lower), hook_count: 1,
  hook: "read-only capture of Plt3 immediately before NonExportsList deletion"}))
end
started = System.monotonic_time(:nanosecond)
warnings = :dialyzer.run(analysis_type: :plt_add, get_warnings: true,
  files: Enum.map(config["beams"], &String.to_charlist/1),
  init_plt: String.to_charlist(config["plt"]), output_plt: String.to_charlist(output_plt),
  check_plt: false, warnings: (if "--unknown" in options, do: [], else: [:no_unknown]) ++ Enum.map(config["warning_options"], &String.to_atom/1),
  error_location: :column, indent_opt: false)
elapsed = System.monotonic_time(:nanosecond) - started
plt = :dialyzer_cplt.from_file(String.to_charlist(output_plt))
entries = try do
  for module <- modules,
      {:value, entries} = :dialyzer_plt.lookup_module(plt, module),
      {{^module, function, arity}, result, args} <- entries,
      do: %{module: Atom.to_string(module), function: Atom.to_string(function), arity: arity,
        result: NativeTypeWire.encode(result), arguments: Enum.map(args, &NativeTypeWire.encode/1)}
after
  :dialyzer_plt.delete(plt)
end
entries = Enum.sort_by(entries, &{&1.module, &1.function, &1.arity})
after_hashes = Map.new(inputs, fn p -> {p, hash.(p)} end)
true = before == after_hashes
File.write!(Path.join(out, "otp-signatures.json"), :json.encode(%{signatures: entries}))
File.write!(Path.join(out, "otp-warnings.etf"), :erlang.term_to_binary(warnings))
defmodule WarningWire do
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
File.write!(Path.join(out, "otp-warnings.json"), :json.encode(%{warnings: WarningWire.term(warnings), warning_count: length(warnings)}))
provenance = %{method: "OTP public dialyzer.run plt_add with get_warnings true; fresh output PLT; target MFA entries only",
  input_sha256: before, inputs_unchanged: true, output_plt_sha256: hash.(output_plt),
  target_module_count: length(modules), signature_count: length(entries), warning_count: length(warnings),
  analysis_ns: elapsed, otp: System.otp_release(), erts: List.to_string(:erlang.system_info(:version)), elixir: System.version()}
File.write!(Path.join(out, "oracle-provenance.json"), :json.encode(provenance))
IO.puts(:json.encode(Map.drop(provenance, [:input_sha256])))
