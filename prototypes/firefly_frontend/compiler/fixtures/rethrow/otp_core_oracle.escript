#!/usr/bin/env escript
main([Path]) ->
    {ok, _Module, Core} = compile:file(Path, [to_core, binary, no_copt]),
    io:format("OTP ~s~n", [erlang:system_info(otp_release)]),
    cerl_trees:fold(fun(Node, Acc) ->
        case cerl:type(Node) of
            primop ->
                case cerl:concrete(cerl:primop_name(Node)) of
                    raise -> io:format("GENERATED_RETHROW ~p~n", [cerl:primop_args(Node)]);
                    _ -> ok
                end;
            call ->
                case {cerl:is_literal(cerl:call_module(Node)), cerl:is_literal(cerl:call_name(Node))} of
                    {true, true} ->
                        case {cerl:concrete(cerl:call_module(Node)),cerl:concrete(cerl:call_name(Node))} of
                            {erlang,raise} -> io:format("EXPLICIT_RAISE ~p~n", [cerl:call_args(Node)]);
                            _ -> ok
                        end;
                    _ -> ok
                end;
            'try' -> io:format("TRY_EVARS ~p~n", [cerl:try_evars(Node)]);
            _ -> ok
        end,
        Acc
    end, ok, Core).
