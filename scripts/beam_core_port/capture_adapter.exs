[repo, fixture_dir | modules] = System.argv()
Code.require_file("core_export.exs", Path.join(repo,"scripts"))
Code.require_file("core_native_export.exs",Path.join(repo,"scripts"))
for name <- modules do
  forms=fixture_dir |> Path.join(name<>".forms.etf") |> File.read!() |> :erlang.binary_to_term()
  core=fixture_dir |> Path.join(name<>".clean_core.etf") |> File.read!() |> :erlang.binary_to_term()
  {record,atoms}=NativeCoreTransport.build(String.to_atom(name),forms,core)
  layout=CompilerAtomPrimitives.export(atoms++[:top,:external,:in,:out,:"$e",:"$v"],"independent-adapter-oracle:"<>name)
  File.write!(Path.join(fixture_dir,name<>".native.etf"),:erlang.term_to_binary(record))
  File.write!(Path.join(fixture_dir,name<>".layout.json"),:json.encode(layout))
end
