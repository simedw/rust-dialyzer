Code.require_file("../core_export.exs", __DIR__)
[out] = System.argv()
beams = Path.join(out, "beams")
File.mkdir_p!(beams)
for source <- Path.wildcard(Path.join(__DIR__, "cases/*.erl")) do
  case :compile.file(String.to_charlist(source), [:debug_info, {:outdir, String.to_charlist(beams)}, :return_errors, :return_warnings]) do
    {:ok, name, _warnings} ->
      module = CoreExport.module(Path.join(beams, "#{name}.beam"))
      File.write!(Path.join(out, "#{name}.json"), :json.encode(%{version: 6, profile: CoreExport.profile(), modules: [module]}))
    error -> raise "Preparation fixture compilation failed: #{inspect(error)}"
  end
end
