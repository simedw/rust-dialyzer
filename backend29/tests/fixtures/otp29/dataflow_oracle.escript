#!/usr/bin/env escript
-mode(compile).
main([]) ->
    "29" = erlang:system_info(otp_release),
    {ok,{_,[{abstract_code,{raw_abstract_v1,Forms0}}]}} = beam_lib:chunks(code:which(dialyzer_dataflow), [abstract_code]),
    Forms1 = [case F of {attribute,A,module,_} -> {attribute,A,module,native_record_dataflow_oracle}; _ -> F end || F <- Forms0],
    Wrapper = "oracle_state(Records) -> #state{module=fixture,records=Records,warning_mode=false,callgraph=dialyzer_callgraph:new(),curr_fun={fixture,fixture,0},codeserver=dialyzer_codeserver:new()}. ",
    {ok,Tokens,_} = erl_scan:string(Wrapper), {ok,StateForm} = erl_parse:parse_form(Tokens),
    {ok,M,Bin,_} = compile:forms(Forms1 ++ [StateForm], [binary,export_all,return_warnings]),
    {module,M} = code:load_binary(M,"dataflow_oracle",Bin),
    Int = erl_types:t_integer(), Bool = erl_types:t_boolean(),
    Records = #{{native_record,{external,entry}} => {{"fixture.erl",1},[{value,{type,0,any,[]},Int}]}, {import_record,entry} => external},
    State = M:oracle_state(Records),
    X = cerl:ann_c_var([{label,100}], 'X'),
    Pair = fun(V) -> cerl:c_record_pair(cerl:c_atom(value), V) end,
    Pattern = fun(Id,V) -> cerl:c_record(cerl:abstract(Id), [Pair(V)]) end,
    Record = fun(T) -> erl_types:t_record({external,entry}, [{value,T}]) end,
    Fresh = M:mark_as_fresh([X],M:map__new()),
    Rows = [
        bind(M, anonymous, Pattern([],X), erl_types:t_atom(impossible), Fresh, State, X),
        bind(M, field_noop, Pattern(entry,X), erl_types:t_any(), M:enter_type(X,erl_types:t_integer(1),Fresh), State, X),
        bind(M, field_failure, Pattern(entry,cerl:c_atom(bad)), erl_types:t_any(), Fresh, State, X),
        guard(M, is_record_positive, call(is_record,[X,cerl:c_atom(external),cerl:c_atom(entry)]), erl_types:t_any(), pos, State, X),
        guard(M, is_record_mismatch, call(is_record,[X,cerl:c_atom(external),cerl:c_atom(entry)]), erl_types:t_atom(other), dont_know, State, X),
        guard(M, is_record_negative, call(is_record,[X,cerl:c_atom(external),cerl:c_atom(entry)]), Record(Bool), neg, State, X),
        guard(M, field_positive, primop(record_field,[X,cerl:c_atom(entry),cerl:c_atom(value)]), erl_types:t_any(), pos, State, X),
        guard(M, field_false, primop(record_field,[X,cerl:c_atom(entry),cerl:c_atom(value)]), Record(erl_types:t_atom(false)), pos, State, X),
        guard(M, other_primop, primop(is_native_record,[X]), erl_types:t_none(), pos, State, X)
    ],
    Dir = filename:dirname(escript:script_name()),
    ok = file:write_file(filename:join(Dir,"dataflow_oracle.json"),json:encode(#{otp => list_to_binary(erlang:system_info(otp_release)), rows => Rows})),
    io:format("captured ~p native dataflow cases~n",[length(Rows)]).
call(Name,Args) -> cerl:ann_c_call([1,{file,"fixture.erl"},{label,101}],cerl:c_atom(erlang),cerl:c_atom(Name),Args).
primop(Name,Args) -> cerl:ann_c_primop([1,{file,"fixture.erl"},{label,101}],cerl:c_atom(Name),Args).
text(T) -> unicode:characters_to_binary(erl_types:t_to_string(T)).
bind(M,Id,Pat,Type,Map,State,X) ->
    try M:bind_pat_vars([Pat],[Type],Map,State,false) of
        {Map1,[Result],_} -> #{id => Id,status => ok,type => text(Result),binding => text(M:lookup_type(X,Map1))};
        {error,Kind,_,Actual} -> #{id => Id,status => failure,kind => Kind,type => text(Actual)}
    catch throw:{error,Kind,_,Actual} -> #{id => Id,status => failure,kind => Kind,type => text(Actual)} end.
guard(M,Id,Tree,Type,Eval,State,X) ->
    Map = M:enter_type(X,Type,M:map__new()),
    try M:bind_guard(Tree,Map,#{},Eval,State) of
        {Map1,Result,_} -> #{id => Id,status => ok,type => text(Result),binding => text(M:lookup_type(X,Map1))}
    catch throw:{fail,none} -> #{id => Id,status => failure} end.
