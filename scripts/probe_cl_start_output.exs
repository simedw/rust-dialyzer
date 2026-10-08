[bridge_script, binary, prior, out | files] = System.argv()
Code.require_file("operational_boundary.exs", Path.dirname(bridge_script))
File.mkdir_p!(out)
source = Path.join(out, "classic_options.erl")
File.write!(source, ~S"""
-module(classic_options).
-include_lib("dialyzer/src/dialyzer.hrl").
-export([make/3,project/1]).
make(Prior,Files,Output) ->
  O=dialyzer_options:build([{analysis_type,succ_typings},{files,Files},{init_plt,Prior},
    {check_plt,false},{erlang_mode,true},{report_mode,quiet},{warnings,[no_unknown]}]),
  O#options{output_plt=Output}.
project(O) -> #{entry => <<"cl_start">>,operation => atom_to_binary(O#options.analysis_type),
  files=>[list_to_binary(P)||P<-O#options.files],apps=>[],priors=>[list_to_binary(P)||P<-O#options.init_plts],
  output=>list_to_binary(O#options.output_plt),get_warnings=>O#options.get_warnings,
  legal_warnings=>[atom_to_binary(T)||T<-O#options.legal_warnings],use_contracts=>O#options.use_contracts,
  check_prior=>O#options.check_plt,location=>atom_to_binary(O#options.error_location),
  solvers=>[atom_to_binary(S)||S<-O#options.solvers]}.
""")
{:ok, :classic_options, bytes} = :compile.file(String.to_charlist(source), [:binary])
{:module, :classic_options} = :code.load_binary(:classic_options, String.to_charlist(source), bytes)
port = Port.open({:spawn_executable, String.to_charlist(binary)}, [:binary, :exit_status, {:line, 100_000_000}])
Process.put(:operation_port, port)
opts = :classic_options.make(String.to_charlist(prior), Enum.map(files, &String.to_charlist/1), String.to_charlist(Path.join(out, "native.plt")))
Process.put(:operation_mode, :native)
native = try do
  %{status: "returned", value: inspect(RustClassicBoundary.start(opts))}
catch class, reason ->
  %{status: "raised", class: Atom.to_string(class), reason: inspect(reason, limit: :infinity)}
end
rust = opts |> :classic_options.project() |> RustClassicBoundary.request()
report = %{entry: "cl_start", exploratory_probe: true, parity_claimed: false,
  native: native, rust: rust, output_exists: File.exists?(Path.join(out, "native.plt"))}
File.write!(Path.join(out, "report.json"), :json.encode(report))
IO.puts(:json.encode(report))
Port.close(port)
