# Runs OTP Dialyzer the way dialyxir does and prints warnings in dialyxir's raw
# format (one `inspect(warning, limit: :infinity)` per line).
#
#   elixir run_dialyzer.exs build --plt OUT --beams LIST [--base PLT] [--seed N]
#   elixir run_dialyzer.exs check --plt PLT --beams LIST --out WARNINGS [--seed N]
#
# LIST is a file with one .beam path per line. `--seed` shuffles the file order
# handed to Dialyzer. `--base` extends an existing PLT (dialyxir's layering).
# Timings and counts go to stderr as one JSON object.

{opts, [mode], []} =
  OptionParser.parse(System.argv(),
    strict: [plt: :string, beams: :string, base: :string, out: :string, seed: :integer]
  )

beams =
  opts[:beams]
  |> File.read!()
  |> String.split("\n", trim: true)
  |> Enum.map(&String.to_charlist/1)

beams =
  case opts[:seed] do
    nil ->
      beams

    seed ->
      :rand.seed(:exsss, {seed, seed, seed})
      Enum.shuffle(beams)
  end

plt = String.to_charlist(opts[:plt])

dialyzer_opts =
  case mode do
    "build" ->
      base =
        case opts[:base] do
          nil -> [analysis_type: :plt_build]
          base -> [analysis_type: :plt_add, init_plt: String.to_charlist(base)]
        end

      base ++ [files: beams, output_plt: plt, warnings: [:unknown]]

    "check" ->
      [check_plt: false, init_plt: plt, files: beams, warnings: [:unknown]]
  end

{micros, warnings} = :timer.tc(fn -> :dialyzer.run(dialyzer_opts) end)

if mode == "check" do
  File.write!(opts[:out], Enum.map(warnings, &[inspect(&1, limit: :infinity), ?\n]))
end

IO.puts(
  :stderr,
  :json.encode(%{
    mode: mode,
    files: length(beams),
    warnings: length(warnings),
    seconds: micros / 1_000_000,
    schedulers: :erlang.system_info(:schedulers_online)
  })
)
