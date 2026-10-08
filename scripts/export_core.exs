Code.require_file("core_export.exs", __DIR__)
case System.argv() do
  [beam_dir, output] -> IO.inspect(CoreExport.run(beam_dir, output))
  _ -> raise("usage: elixir scripts/export_core.exs BEAM_DIR OUTPUT.json")
end
