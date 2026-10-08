Code.require_file("native_types.exs", __DIR__)
Code.ensure_loaded!(:erl_types)
for line <- IO.stream(:stdio, :line) do
  request=:json.decode(line)
  arguments=Enum.map(request["arguments"], &NativeTypes.decode/1)
  result=try do
    %{ok: true,result: NativeTypes.request(request)}
  catch
    :error, reason ->
      error=case reason do
        :function_clause -> %{kind: "error",class: "function_clause"}
        :badarg -> %{kind: "error",class: "badarg"}
        {:case_clause, []} -> %{kind: "error",class: "case_clause",payload: %{kind: "nil"}}
        {:case_clause, value} -> %{kind: "error",class: "case_clause",payload: %{kind: "type",value: NativeTypes.term(value)}}
        other -> %{kind: "unclassified",reason: NativeTypes.term(other)}
      end
      %{ok: false,native_error: error,arguments: Enum.map(arguments,&NativeTypes.term/1)}
  end
  IO.puts(:json.encode(result))
end
