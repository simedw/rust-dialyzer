# One cold OTP analysis over exactly the frozen BEAM inventory used by Rust.
[root, inventory, out | flags] = System.argv()
files = File.read!(Path.join(inventory, "beams.txt")) |> String.split("\n", trim: true) |> Enum.map(&String.to_charlist/1)
options = File.read!(Path.join(inventory, "options.json")) |> :json.decode()
if "--capture" in flags do
  Code.require_file("type_wire.exs", Path.join(root, "scripts"))
  modules = options["report_modules"] || Enum.map(files, fn path -> path |> List.to_string() |> Path.basename(".beam") end)
  :persistent_term.put(:cold_signature_capture, {MapSet.new(modules, &String.to_atom/1), out})
  defmodule ColdSignatureCapture do
    def capture(plt) do
      {modules, out} = :persistent_term.get(:cold_signature_capture)
      rows = for {{m, f, a}, {result, args}} <- :ets.tab2list(elem(plt, 1)), MapSet.member?(modules, m) do
        %{module: Atom.to_string(m), function: Atom.to_string(f), arity: a,
          result: NativeTypeWire.encode(result), arguments: Enum.map(args, &NativeTypeWire.encode/1)}
      end
      File.write!(Path.join(out, "otp-full-signatures.json"), :json.encode(%{signatures: Enum.sort_by(rows, &{&1.module, &1.function, &1.arity})}))
    end
    def walk({:match, ann, {:var, _, :Plt4}, {:call, _, {:remote, _, {:atom, _, :dialyzer_plt}, {:atom, _, :delete_list}}, [{:var, _, :Plt3}, {:var, _, :NonExportsList}]}} = node) do
      Process.put(:cold_capture_sites, Process.get(:cold_capture_sites, 0) + 1)
      call = {:call, ann, {:remote, ann, {:atom, ann, __MODULE__}, {:atom, ann, :capture}}, [{:var, ann, :Plt3}]}
      {:block, ann, [call, node]}
    end
    def walk(term) when is_tuple(term), do: term |> Tuple.to_list() |> Enum.map(&walk/1) |> List.to_tuple()
    def walk(term) when is_list(term), do: Enum.map(term, &walk/1)
    def walk(term), do: term
  end
  module = :dialyzer_analysis_callgraph
  path = :code.which(module)
  {:ok, {^module, [abstract_code: {:raw_abstract_v1, forms}]}} = :beam_lib.chunks(path, [:abstract_code])
  forms = ColdSignatureCapture.walk(forms)
  1 = Process.get(:cold_capture_sites)
  {:ok, ^module, binary, _} = :compile.forms(forms, [:binary, :return_errors, :return_warnings])
  {:module, ^module} = :code.load_binary(module, ~c"read-only signature capture", binary)
end
warning_options = if "warn_opaque" in options["legal_warnings"], do: [:unknown], else: [:unknown, :no_opaque]
{micros, warnings} = :timer.tc(fn ->
  common = [check_plt: false, warnings: warning_options, error_location: :column]
  plt = String.to_charlist(Path.join(out, "otp.plt"))
  if options["report_modules"] do
    report = MapSet.new(options["report_modules"])
    {application, dependencies} = Enum.split_with(files, fn path ->
      MapSet.member?(report, path |> List.to_string() |> Path.basename(".beam"))
    end)
    :dialyzer.run(common ++ [analysis_type: :plt_build, get_warnings: false, files: dependencies, output_plt: plt])
    :dialyzer.run(common ++ [analysis_type: :succ_typings, get_warnings: true, files: application, init_plt: plt])
  else
    :dialyzer.run(common ++ [analysis_type: :plt_build, get_warnings: true, files: files, output_plt: plt])
  end
end)
File.write!(Path.join(out, "otp-warnings.etf"), :erlang.term_to_binary(warnings))
defmodule ColdWarningWire do
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
File.write!(Path.join(out, "otp-warnings.json"), :json.encode(%{warnings: ColdWarningWire.term(warnings), warning_count: length(warnings)}))
IO.puts(:json.encode(%{seconds: micros / 1_000_000, files: length(files), warnings: length(warnings), schedulers: :erlang.system_info(:schedulers_online), otp: System.otp_release(), elixir: System.version()}))
