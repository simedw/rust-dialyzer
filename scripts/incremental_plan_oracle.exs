defmodule IncrementalPlanBoundary do
  def files(_), do: []
  def identity(value), do: value
  def check(_, _, _), do: Process.get(:check)
  def new() do
    input = Process.get(:input)
    Process.put(:discard, Enum.uniq(input["previous"] ++ input["current"]) |> Enum.sort())
    Process.put(:remaining, %{})
    Process.put(:reuse, false)
    :empty
  end
  def clean(_, discard) do
    Process.put(:discard, :sets.to_list(discard) |> Enum.map(&Atom.to_string/1) |> Enum.sort())
    :partial
  end
  def dependencies(dict) do
    Map.new(:dict.to_list(dict), fn {module, deps} ->
      {Atom.to_string(module), Enum.map(deps, &Atom.to_string/1) |> Enum.sort()}
    end)
  end
  def analyze(files, _, _, info) do
    Process.put(:remaining, dependencies(elem(info, 2)))
    modules = Enum.map(files, fn file -> file |> List.to_string() |> Path.basename(".beam") |> String.to_atom() end)
    {{0, []}, modules}
  end
  def existing(_, _), do: {{0, []}, []}
  def write(_, _, deps, _) do
    Process.put(:remaining, dependencies(deps))
    :ok
  end
end

defmodule IncrementalPlanSource do
  def rewrite({:call, anno, {:atom, fa, name}, arguments} = call) do
    rewritten = Enum.map(arguments, &rewrite/1)
    cond do
      String.starts_with?(Atom.to_string(name), "report_") or name in [:assert_metrics_file_valid, :write_module_to_path_lookup] -> {:atom, anno, :ok}
      name == :init_opts_for_incremental -> remote(anno, fa, :identity, rewritten)
      name == :get_files_from_opts -> remote(anno, fa, :files, rewritten)
      name == :do_analysis -> remote(anno, fa, :analyze, rewritten)
      name == :return_existing_errors -> remote(anno, fa, :existing, rewritten)
      name == :clean_plt -> remote(anno, fa, :clean, rewritten)
      true -> put_elem(call, 3, rewritten)
    end
  end
  def rewrite({:call, anno, {:remote, _, {:atom, _, module}, {:atom, fa, name}}, args}) when {module, name} in [{:dialyzer_iplt, :check_incremental_plt}, {:dialyzer_iplt, :to_file}, {:dialyzer_plt, :new}] do
    remote(anno, fa, %{check_incremental_plt: :check, to_file: :write, new: :new}[name], Enum.map(args, &rewrite/1))
  end
  def rewrite(tuple) when is_tuple(tuple), do: tuple |> Tuple.to_list() |> Enum.map(&rewrite/1) |> List.to_tuple()
  def rewrite(list) when is_list(list), do: Enum.map(list, &rewrite/1)
  def rewrite(value), do: value
  def remote(anno, fa, name, args), do: {:call, anno, {:remote, anno, {:atom, anno, IncrementalPlanBoundary}, {:atom, fa, name}}, args}
  def load() do
    {:ok, {:dialyzer_incremental, [{:abstract_code, {:raw_abstract_v1, forms}}]}} = :beam_lib.chunks(:code.which(:dialyzer_incremental), [:abstract_code])
    forms = Enum.map(forms, fn
      {:attribute, a, :module, :dialyzer_incremental} -> {:attribute, a, :module, :rust_incremental_plan_oracle}
      {:function, _, :start_report_modules_changed_and_analyzed, _, _} = form -> rewrite(form)
      form -> form
    end)
    {:ok, :rust_incremental_plan_oracle, binary} = :compile.forms(forms, [:binary, :export_all])
    {:module, :rust_incremental_plan_oracle} = :code.load_binary(:rust_incremental_plan_oracle, ~c"incremental_plan_oracle", binary)
  end
end
IncrementalPlanSource.load()
options = :dialyzer_options.build([analysis_type: :incremental, plts: [~c"snapshot"], files: [], report_mode: :quiet])
for line <- IO.stream(:stdio, :line) do
  input = :json.decode(line)
  Process.put(:input, input)
  Process.put(:discard, [])
  Process.put(:remaining, input["dependencies"])
  Process.put(:reuse, true)
  paths = Map.new(input["current"], fn m -> {String.to_atom(m), String.to_charlist("/probe/" <> m <> ".beam")} end)
  md5 = Enum.map(input["current"], fn m -> {String.to_atom(m), <<0::128>>} end)
  deps = :dict.from_list(Enum.map(input["dependencies"], fn {m, ds} -> {String.to_atom(m), Enum.map(ds, &String.to_atom/1) |> Enum.sort()} end))
  diffs = Enum.map(input["changed"], &{:differ, String.to_atom(&1)}) ++ Enum.map(input["removed"], &{:removed, String.to_atom(&1)})
  warnings = %{}
  check = case input["check"] do
    "fresh" -> {:ok, {:iplt_info, md5, deps, warnings, []}, paths}
    "missing_warnings" -> {:ok, {:iplt_info, md5, deps, :none, []}, paths}
    "stale_version" -> {:old_version, md5, paths}
    "missing_file" -> {:new_file, md5, paths}
    "changed" -> {:differ, md5, diffs, deps, warnings, paths}
    "changed_without_warnings" -> {:differ, md5, diffs, deps, :none, paths}
    "warning_profile_changed" -> {:legal_warnings_changed, md5, paths}
  end
  Process.put(:check, check)
  {{0, []}, changed, analyzed} = :rust_incremental_plan_oracle.start_report_modules_changed_and_analyzed(options)
  output = %{analyze: Enum.map(analyzed, &Atom.to_string/1) |> Enum.sort(), changed_or_removed: if(changed == :undefined, do: :null, else: Enum.map(changed, &Atom.to_string/1) |> Enum.sort()), discard: Process.get(:discard), remaining_dependencies: Process.get(:remaining), reuse_stored_warnings: Process.get(:reuse)}
  IO.puts(:json.encode(output))
end
