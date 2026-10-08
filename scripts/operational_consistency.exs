defmodule RustRegistryBoundary do
  def new_tests(directory, declared) do
    request = %{id: "registry", suite_dir: to_string(directory), declared: Enum.map(declared, &Atom.to_string/1)}
    port = Process.get(:registry_port)
    true = Port.command(port, [:json.encode(request), "\n"])
    receive do
      {^port, {:data, {:eol, line}}} ->
        %{"missing" => names} = :json.decode(line)
        Enum.map(names, &String.to_atom/1)
      {^port, {:exit_status, status}} -> raise "registry probe exited #{status}"
    after
      10_000 -> raise "registry probe timed out"
    end
  end
end

defmodule OperationalConsistency do
  def load(forms, name) do
    case :compile.forms(forms, [:binary, :return_errors, :return_warnings]) do
      {:ok, ^name, bytes, _warnings} -> {:module, ^name} = :code.load_binary(name, ~c"isolated operational suite", bytes)
      other -> raise inspect(other)
    end
  end

  def rewrite({:call, a, {:remote, b, {:atom, c, :dialyzer_common}, {:atom, d, :new_tests}}, args}) do
    {:call, a, {:remote, b, {:atom, c, RustRegistryBoundary}, {:atom, d, :new_tests}}, args}
  end
  def rewrite(term) when is_tuple(term), do: term |> Tuple.to_list() |> Enum.map(&rewrite/1) |> List.to_tuple()
  def rewrite(term) when is_list(term), do: Enum.map(term, &rewrite/1)
  def rewrite(term), do: term

  def omit_case(forms, omitted) do
    Enum.map(forms, fn
      {:function, a, :all, 0, [{:clause, b, [], [], [body]}]} ->
        value = :erl_parse.normalise(body)
        replacement = :erl_parse.abstract(List.delete(value, omitted))
        {:function, a, :all, 0, [{:clause, b, [], [], [replacement]}]}
      form -> form
    end)
  end

  def outcome(module, name, config) do
    try do
      %{status: "returned", value: inspect(apply(module, name, [config]))}
    catch
      kind, reason -> %{status: "raised", class: Atom.to_string(kind), reason: inspect(reason, limit: :infinity)}
    end
  end

  def run(root, binary, out) do
    test = Path.join(root, ".cache/upstream/otp/lib/dialyzer/test")
    inventory = :json.decode(File.read!(Path.join(root, "corpus/inventory.json")))
    suites = inventory["otp_generated_suites"]
    File.mkdir_p!(out)
    for helper <- [:file_utils, :dialyzer_common] do
      file = Path.join(test, Atom.to_string(helper) <> ".erl")
      {:ok, forms} = :epp.parse_file(String.to_charlist(file), [], [])
      load(forms, helper)
    end
    for suite <- suites do
      dir = suite["suite"] <> "_SUITE_data"
      File.ln_s!(Path.join(test, dir), Path.join(out, dir))
    end
    File.cd!(out, fn -> :ok = :dialyzer_common.create_all_suites() end)
    port = Port.open({:spawn_executable, String.to_charlist(binary)}, [:binary, :exit_status, {:line, 10_000_000}])
    Process.put(:registry_port, port)
    rows = for suite <- suites do
      name = suite["suite"] <> "_SUITE"
      module = String.to_atom(name)
      file = Path.join(out, name <> ".erl")
      {:ok, forms} = :epp.parse_file(String.to_charlist(file), [String.to_charlist(test)], [])
      config = [{:data_dir, String.to_charlist(Path.join(out, name <> "_data"))}]
      case_name = String.to_atom(name <> "_consistency")
      load(forms, module)
      native = outcome(module, case_name, config)
      omitted = Enum.find(apply(module, :all, []), &(&1 != case_name))
      omitted_forms = omit_case(forms, omitted)
      load(Enum.map(forms, &rewrite/1), module)
      rust = outcome(module, case_name, config)
      load(omitted_forms, module)
      native_missing = outcome(module, case_name, config)
      load(Enum.map(omitted_forms, &rewrite/1), module)
      rust_missing = outcome(module, case_name, config)
      %{id: "consistency/" <> suite["suite"], native: native, rust: rust,
        passed: native == %{status: "returned", value: ":ok"} and rust == native,
        omitted_case: Atom.to_string(omitted), native_missing: native_missing, rust_missing: rust_missing,
        omission_exact: native_missing.status == "raised" and rust_missing == native_missing,
        original_generated_case_executed: true, public_boundary_only_rewritten: true}
    end
    Port.close(port)
    File.write!(Path.join(out, "cases.json"), :json.encode(rows))
    report = %{requested: length(suites), passed: Enum.count(rows, & &1.passed),
      omission_exact: Enum.count(rows, & &1.omission_exact),
      suite_setup: "Case data_dir is supplied directly; PLT-producing init_per_suite is unnecessary for registry-only cases",
      native_target_analysis_as_input: false}
    File.write!(Path.join(out, "report.json"), :json.encode(report))
    IO.puts(inspect(report))
    if report.passed != report.requested or report.omission_exact != report.requested, do: System.halt(1)
  end
end

[root, binary, out] = System.argv()
OperationalConsistency.run(root, binary, out)
