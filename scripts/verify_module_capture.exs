[baseline, candidate, output | beams] = System.argv()
Code.compile_string(baseline |> File.read!() |> String.replace("defmodule CoreExport do", "defmodule BaselineCoreExport do"), baseline)
Code.require_file(candidate)
:logger.set_primary_config(:level, :none)
Enum.each(beams, fn path ->
  old = BaselineCoreExport.module(path)
  captured = CoreExport.capture_module(path)
  checked = CoreExport.module(path)
  if old !== captured or old !== checked, do: raise("complete module capture differs: #{path}")
  {forms, core} = CoreExport.rebuild_module(captured |> :json.encode() |> IO.iodata_to_binary() |> :json.decode())
  {:ok, native} = :dialyzer_utils.get_core_from_beam(String.to_charlist(path))
  {:ok, {_, [abstract_code: {:raw_abstract_v1, native_forms}]}} = :beam_lib.chunks(String.to_charlist(path), [:abstract_code])
  if :erlang.term_to_binary(forms) !== :erlang.term_to_binary(native_forms), do: raise("source forms differ: #{path}")
  if :erlang.term_to_binary(core) !== :erlang.term_to_binary(native), do: raise("Native Core differs: #{path}")
end)
report = %{compiled_modules_exact: length(beams), complete_capture_ids_exact: true,
  checked_path_exact: true, native_core_and_forms_exact: true, native_roundtrips_exact: true}
File.write!(output, :json.encode(report))
IO.puts(:json.encode(report))
