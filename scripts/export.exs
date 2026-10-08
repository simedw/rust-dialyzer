Code.require_file("beam_export.exs", __DIR__)

case System.argv() do
  [beam_dir, output] -> Export.run(beam_dir, output)
  _ -> raise "usage: elixir scripts/export.exs BEAM_DIRECTORY OUTPUT_JSON"
end

