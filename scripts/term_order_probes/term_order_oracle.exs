Code.require_file("../core_export.exs",__DIR__)
[p,r]=System.argv()
checks=Enum.zip(File.stream!(p),File.stream!(r))|>Enum.with_index()|>Enum.map(fn{{c,r},i}->
 c=:json.decode(c);r=:json.decode(r);a=CoreExport.rebuild_terms(c["a"]);b=CoreExport.rebuild_terms(c["b"])
 {a,b}=if c["exact"],do: {%{a=>:value},%{b=>:value}},else: {a,b}
 expected=cond do a<b->-1;a>b->1;true->0 end
 %{id: i,parity: r["ok"]===expected,expected: expected,rust: r}
end)
failures=Enum.reject(checks,& &1.parity);report=%{outcomes: length(checks),matched: length(checks)-length(failures),failed: length(failures),failures: failures}
File.write!(r<>".comparison.json",:json.encode(report));IO.inspect(Map.drop(report,[:failures]));if failures != [],do: raise("term ordering mismatch")
