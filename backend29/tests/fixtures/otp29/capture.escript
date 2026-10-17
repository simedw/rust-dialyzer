#!/usr/bin/env escript
%% Run with the authenticated OTP29 escript after compiling the three .erl files.
main([]) ->
    "29" = erlang:system_info(otp_release),
    Dir = filename:dirname(escript:script_name()),
    Opts = [no_copt,to_core,binary,return_errors,no_inline,strict_record_tests,
            dialyzer,no_spawn_compiler_process],
    Modules = [native_record_dependency29,native_records29,native_comprehensions29],
    Cases = [capture(Dir, M, Opts) || M <- Modules],
    Binaries = [{M, crypto:hash(sha256, element(2, file:read_file(code:which(M))))}
                || M <- [compile,v3_core,erl_expand_records,dialyzer_clean_core]],
    Metadata = #{otp => erlang:system_info(otp_release),
                 erts => erlang:system_info(version), options => Opts,
                 compiler_beam_sha256 => Binaries, cases => Cases},
    file:write_file(filename:join(Dir,"oracle_metadata.term"),
                    io_lib:format("~p.~n",[Metadata])).
capture(Dir, Module, Opts) ->
    Base = filename:join(Dir, atom_to_list(Module)),
    %% epp strips -feature attributes. debug_info retains the feature operation
    %% but compile:noenv_forms needs epp's final internal feature list as well.
    CoreOpts = case Module of native_comprehensions29 -> [{features,[compr_assign]}|Opts]; _ -> Opts end,
    {ok,{Module,[{debug_info,{debug_info_v1,Backend,Data}}]}} =
        beam_lib:chunks(Base ++ ".beam", [debug_info]),
    {ok,Raw} = case Backend:debug_info(core_v1, Module, Data, CoreOpts) of
        {ok,_}=Ok -> Ok;
        Error -> erlang:error({Module,Error})
    end,
    Clean = dialyzer_clean_core:clean(Raw),
    ok = file:write_file(Base ++ ".raw_core.etf", term_to_binary(Raw)),
    ok = file:write_file(Base ++ ".clean_core.etf", term_to_binary(Clean)),
    {ok, Source} = file:read_file(Base ++ ".erl"),
    {ok, Beam} = file:read_file(Base ++ ".beam"),
    #{module => Module, core_options => CoreOpts, source_sha256 => crypto:hash(sha256, Source),
      input_beam_sha256 => crypto:hash(sha256, Beam)}.
