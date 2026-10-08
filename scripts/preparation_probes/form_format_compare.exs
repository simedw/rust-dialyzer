Code.require_file("../core_export.exs",__DIR__)
[dir]=System.argv()
n=File.stream!(Path.join(dir,"native.jsonl"))|>Enum.map(&:json.decode/1)
r=File.stream!(Path.join(dir,"rust.jsonl"))|>Enum.map(&:json.decode/1)
if length(n)!=length(r),do: raise("row count differs")
rows=Enum.zip(n,r)|>Enum.with_index()|>Enum.map(fn {{n,r},i}->
 pass=if n["ok"],do: r["ok"] && CoreExport.rebuild_terms(n["ok"])===CoreExport.rebuild_terms(r["ok"]),else: r["error"] && n["kind"]==r["error"]["kind"] && CoreExport.rebuild_terms(n["reason"])===CoreExport.rebuild_terms(r["error"]["reason"])
 %{index: i,pass: pass==true,native: n,rust: r}
end)
report=%{cases: length(rows),matches: Enum.count(rows,& &1.pass),failures: Enum.filter(rows,&(!&1.pass))}
File.write!(Path.join(dir,"report.json"),:json.encode(report));IO.inspect(Map.drop(report,[:failures]));if report.matches != report.cases,do: System.halt(1)
