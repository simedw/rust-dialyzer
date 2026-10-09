# Builds or reuses a per-module Core Erlang cache with OTP's own compiler.
#
#   elixir core_cache.exs REPO BEAM_LIST CACHE_DIR MANIFEST [--jobs N]
#
# Dialyzer analyzes Core Erlang that it compiles from each module's debug_info.
# This does the same compilation (same options, same cleaning), once per BEAM
# content hash. Entries live under CACHE_DIR/<profile>, where <profile> hashes
# the OTP/Elixir/compiler profile, so upgrading the toolchain never reuses stale
# Core. Each entry is the compact ETF record read by src/core/etf.rs.
#
# MANIFEST receives the analysis context (profile, atom layout, closure
# profiles) and the cached module file for every BEAM, in input order.

{opts, [repo, beam_list, cache_dir, manifest], []} =
  OptionParser.parse(System.argv(), strict: [jobs: :integer])

Code.require_file("core_export.exs", Path.join(repo, "scripts"))
Code.require_file("core_native_export.exs", Path.join(repo, "scripts"))

defmodule CoreCache do
  @options [:no_copt, :to_core, :binary, :return_errors, :no_inline,
            :strict_record_tests, :dialyzer, :no_spawn_compiler_process]

  def entry(path, dir) do
    bytes = File.read!(path)
    key = :crypto.hash(:sha256, bytes) |> Base.encode16(case: :lower)
    core_file = Path.join(dir, key <> ".etf")
    atoms_file = Path.join(dir, key <> ".atoms")

    if File.exists?(core_file) and File.exists?(atoms_file) do
      %{beam: path, core: core_file, atoms: read_atoms(atoms_file), compiled: false}
    else
      {exported, atoms} = compile(bytes)
      atoms = Enum.map(atoms, &Atom.to_string/1)
      write_atomically(core_file, :erlang.term_to_binary(exported))
      write_atomically(atoms_file, :erlang.term_to_binary(atoms))
      %{beam: path, core: core_file, atoms: atoms, compiled: true}
    end
  end

  defp compile(bytes) do
    {:ok, {name, [debug_info: {:debug_info_v1, backend, metadata}, abstract_code: {:raw_abstract_v1, forms}]}} =
      :beam_lib.chunks(bytes, [:debug_info, :abstract_code])
    {:ok, unclean} = backend.debug_info(:core_v1, name, metadata, @options)
    NativeCoreTransport.build(name, forms, :dialyzer_clean_core.clean(unclean))
  end

  defp read_atoms(file), do: file |> File.read!() |> :erlang.binary_to_term()

  # Concurrent runs may race on the same key; rename keeps every file whole.
  defp write_atomically(file, data) do
    tmp = "#{file}.#{System.unique_integer([:positive])}.tmp"
    File.write!(tmp, data)
    File.rename!(tmp, file)
  end

  # FunT table of a Dialyzer module: closure identities the analyzer reproduces.
  def fun_profile(name) do
    path = :code.which(name)
    {:ok, ^name, chunks} = :beam_lib.all_chunks(path)
    {:ok, {^name, [atoms: atoms]}} = :beam_lib.chunks(path, [:atoms])
    atoms = Map.new(atoms)
    {_, table} = List.keyfind(chunks, ~c"FunT", 0)
    <<count::32, entries::binary>> = table
    rows = for <<name::32, arity::32, label::32, index::32, free::32, uniq::32 <- entries>>,
      do: %{name: Atom.to_string(Map.fetch!(atoms, name)), arity: arity, label: label,
        index: index, env_count: free, uniq: uniq}
    true = length(rows) == count
    %{module: Atom.to_string(name),
      original_sha256: Base.encode16(:crypto.hash(:sha256, File.read!(List.to_string(path)))),
      fun_table: Enum.with_index(rows, fn row, i -> Map.put(row, :new_index, i) end)}
  end
end

Application.load(:dialyzer)
profile = CoreExport.profile()
canonical_profile = profile
  |> Enum.map(fn {key, value} -> {Atom.to_string(key), value} end)
  |> Enum.sort_by(&elem(&1, 0))
profile_hash = :crypto.hash(:sha256,
  :erlang.term_to_binary({"native-core-v1-flat-fallback-depth192", canonical_profile}))
  |> Base.encode16(case: :lower) |> binary_part(0, 16)
profile_key = "native-core-v1-flat-fallback-depth192-" <> profile_hash
dir = Path.join(cache_dir, profile_key)
File.mkdir_p!(dir)
beams = beam_list |> File.read!() |> String.split("\n", trim: true)

{micros, entries} =
  :timer.tc(fn ->
    beams
    |> Task.async_stream(&CoreCache.entry(&1, dir),
      max_concurrency: opts[:jobs] || System.schedulers_online(), timeout: :infinity)
    |> Enum.map(fn {:ok, entry} -> entry end)
  end)

atoms = entries |> Enum.flat_map(& &1.atoms) |> Enum.map(&String.to_atom/1)
layout = CompilerAtomPrimitives.export(atoms ++ [:top, :external, :in, :out, :"$e", :"$v"],
  Base.encode16(:crypto.strong_rand_bytes(16)))

File.write!(manifest, :json.encode(%{
  context: %{version: 6, profile: profile, atom_layout: layout,
    contract_closure_profile: CoreCache.fun_profile(:dialyzer_contracts)},
  closure_profile: CoreCache.fun_profile(:dialyzer_typesig),
  module_files: Enum.map(entries, & &1.core)
}))

compiled = Enum.count(entries, & &1.compiled)
IO.puts(:json.encode(%{modules: length(entries), compiled: compiled, reused: length(entries) - compiled,
  seconds: micros / 1_000_000, cache: dir}))
