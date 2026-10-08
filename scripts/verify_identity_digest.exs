[baseline, candidate_root, output] = System.argv()
Code.require_file("dependency_export.exs", Path.dirname(baseline))
Code.require_file("analysis_profile_export.exs", Path.dirname(baseline))
source = File.read!(baseline)
start = :binary.match(source, "defmodule CompilerBundle do") |> elem(0)
finish = :binary.match(source, "[operation, request_path, output_path] = System.argv()") |> elem(0)
Code.compile_string(binary_part(source, start, finish - start), baseline)
Code.require_file("compiler_identity.exs", Path.join(candidate_root, "scripts"))
files = [Path.join(candidate_root, "target/release/rust-dialyzer-poc") | Path.wildcard(Path.join(candidate_root, "scripts/*.exs"))]
inputs = [files, Enum.reverse(files) ++ [hd(files)], Enum.map(files, &Path.relative_to_cwd/1)]
rows = Enum.map(inputs, fn files ->
  request = %{"implementation_files" => files}
  original = CompilerBundle.implementation_identity(request)
  actual = CompilerIdentity.digest(request)
  if original !== actual, do: raise("implementation digest differs from old Native function")
  %{exact: true, digest: actual, inputs: length(files)}
end)
if CompilerBundle.implementation_inputs() !== CompilerIdentity.implementation_inputs(),
  do: raise("Native implementation fingerprint paths differ")
report = %{native_identity_digest_exact: true, implementation_inputs_exact: true, cases: rows}
File.write!(output, :json.encode(report))
IO.puts(:json.encode(report))
