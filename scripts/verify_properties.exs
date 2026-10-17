Code.require_file("type_wire.exs", __DIR__)

defmodule ProperGate do
  def command(name, input) do
    values = if name in [:associativity, :absorption, :commutativity], do: Tuple.to_list(input), else: [input]

    Enum.zip(["a", "b", "c"], values)
    |> Enum.reduce(%{"op" => "property", "property" => to_string(name)}, fn {key, type}, acc ->
      Map.put(acc, key, NativeTypeWire.encode(type))
    end)
  end

  def native(name, input) do
    args = if name in [:associativity, :absorption, :commutativity], do: Tuple.to_list(input), else: [input]

    try do
      apply(:erl_types_prop, String.to_atom("#{name}_check"), args)
    catch
      kind, reason -> {kind, reason}
    end
  end

  def evaluate(port, name, input, commands, results) do
    command = command(name, input)
    true = Port.command(port, IO.iodata_to_binary(:json.encode(command)) <> "\n")

    result =
      receive do
        {^port, {:data, {:eol, line}}} -> :json.decode(line)
        {^port, {:exit_status, status}} -> raise("Rust probe exited #{status}")
      after
        10_000 -> raise("Rust probe timed out")
      end

    :ok = IO.binwrite(commands, [:json.encode(command), "\n"])
    :ok = IO.binwrite(results, [:json.encode(result), "\n"])
    native = native(name, input)

    %{
      passed: native == true and result["ok"] == true,
      native: inspect(native, limit: :infinity),
      rust: result,
      command: command
    }
  end

  def shrink(port, name, imm, type, commands, results, state, attempts) when attempts < 500 do
    case :proper_shrink.shrink(imm, type, state) do
      {[], :done} ->
        {imm, attempts}

      {[], next} ->
        shrink(port, name, imm, type, commands, results, next, attempts + 1)

      {candidates, next} ->
        failure =
          candidates
          |> Enum.with_index(1)
          |> Enum.find(fn {candidate, _} ->
            not evaluate(port, name, :proper_gen.clean_instance(candidate), commands, results).passed
          end)

        case failure do
          nil ->
            shrink(port, name, imm, type, commands, results, next, attempts + 1)

          {candidate, index} ->
            shrink(
              port,
              name,
              candidate,
              type,
              commands,
              results,
              {:shrunk, index, next},
              attempts + 1
            )
        end
    end
  end

  def shrink(_port, _name, imm, _type, _commands, _results, _state, attempts), do: {imm, attempts}

  def run(proper_dir, otp_root, out, binary, count) do
    File.mkdir_p!(out)
    :code.add_patha(String.to_charlist(Path.join(proper_dir, "ebin")))
    :code.add_patha(String.to_charlist(out))
    source = Path.join(otp_root, "lib/dialyzer/test/property_test/erl_types_prop.erl")

    flags = [
      :debug_info,
      {:d, :PROPER},
      {:i, String.to_charlist(Path.join(proper_dir, "include"))},
      {:outdir, String.to_charlist(out)}
    ]

    result = :compile.file(String.to_charlist(source), flags)

    unless match?({:ok, :erl_types_prop}, result),
      do: raise("upstream property compile failed #{inspect(result)}")

    port =
      Port.open(
        {:spawn_executable,
         String.to_charlist(binary)},
        [:binary, :exit_status, {:line, 10_000_000}]
      )

    {:ok, commands} = File.open(Path.join(out, "commands.jsonl"), [:write, :binary])
    {:ok, results} = File.open(Path.join(out, "results.jsonl"), [:write, :binary])
    properties = [:absorption, :associativity, :commutativity, :idempotence, :identity, :limit]

    reports =
      properties
      |> Enum.with_index(1)
      |> Enum.map(fn {name, property_index} ->
        started = System.monotonic_time(:millisecond)

        generator =
          if name == :associativity,
            do: :erl_types_prop.type_without_no_return(),
            else: :erl_types_prop.type()

        arity =
          case name do
            :associativity -> 3
            name when name in [:absorption, :commutativity] -> 2
            _ -> 1
          end

        raw =
          if arity == 1,
            do: generator,
            else: :proper_types.tuple(List.duplicate(generator, arity))

        type = :proper_types.cook_outer(raw)

        {passed, failures} =
          Enum.reduce_while(1..count, {0, []}, fn index, {passed, failures} ->
            size = 1 + div((index - 1) * 99, max(count - 1, 1))
            seed = {28306, property_index, index}
            :ok = :proper.global_state_init_size_seed(size, seed)
            {:ok, imm} = :proper_gen.safe_generate(type)
            input = :proper_gen.clean_instance(imm)
            observed = evaluate(port, name, input, commands, results)

            if observed.passed do
              :proper.global_state_erase()
              if rem(index, 500) == 0, do: IO.puts("#{name}: #{index}/#{count}")
              {:cont, {passed + 1, failures}}
            else
              File.write!(Path.join(out, "#{name}_original.etf"), :erlang.term_to_binary(input))
              {minimal, attempts} = shrink(port, name, imm, type, commands, results, :init, 0)
              minimal_input = :proper_gen.clean_instance(minimal)

              File.write!(
                Path.join(out, "#{name}_minimal.etf"),
                :erlang.term_to_binary(minimal_input)
              )

              failure = %{
                index: index,
                seed: Tuple.to_list(seed),
                size: size,
                original: observed,
                minimal: evaluate(port, name, minimal_input, commands, results),
                shrink_attempts: attempts
              }

              :proper.global_state_erase()
              {:halt, {passed, [failure | failures]}}
            end
          end)

        report = %{
          property: to_string(name),
          requested: count,
          passed: passed,
          failures: failures,
          elapsed_ms: System.monotonic_time(:millisecond) - started,
          seed_formula: "{28306, property index (1-6), case index (1-5000)}",
          generator:
            if(name == :associativity,
              do: "erl_types_prop:type_without_no_return/0",
              else: "erl_types_prop:type/0"
            ),
          native_check: "erl_types_prop:#{name}_check/#{arity}"
        }

        File.write!(Path.join(out, "#{name}.json"), :json.encode(report))
        IO.puts("#{name}: #{passed}/#{count}; #{length(failures)} failures")
        report
      end)

    File.close(commands)
    File.close(results)
    Port.close(port)

    report = %{
      properties: reports,
      property_source: Path.expand(source),
      property_source_sha256: Base.encode16(:crypto.hash(:sha256, File.read!(source)), case: :lower),
      proper_tag: "v1.5.0",
      proper_sha: "5640d0715d4b346676267504d8e84398e2a29f75",
      proper_app_version: "1.4.0",
      runtime: %{
        otp: to_string(:erlang.system_info(:otp_release)),
        erts: to_string(:erlang.system_info(:version))
      },
      unsupported: 0
    }

    File.write!(Path.join(out, "report.json"), :json.encode(report))
    if Enum.any?(reports, &(&1.failures != [])), do: System.halt(1)
  end
end

[proper, otp, out, binary | rest] = System.argv()
ProperGate.run(proper, otp, out, binary, if(rest == [], do: 5000, else: String.to_integer(hd(rest))))
