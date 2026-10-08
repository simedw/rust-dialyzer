Code.require_file("../core_export.exs",__DIR__)
[dir]=System.argv();ns=File.stream!(Path.join(dir,"native.jsonl"))|>Enum.map(&:json.decode/1);rs=File.stream!(Path.join(dir,"rust.jsonl"))|>Enum.map(&:json.decode/1);if length(ns)!=length(rs),do: raise("count mismatch")
rows=Enum.zip(ns,rs)|>Enum.with_index()|>Enum.map(fn {{n,r},i}->
 pass=if Map.has_key?(n,"ok"),do: n["ok"]===r["ok"],else: r["error"] && n["kind"]==r["error"]["kind"] && CoreExport.rebuild_terms(n["reason"])===CoreExport.rebuild_terms(r["error"]["reason"])
 %{index: i,pass: pass==true,native: n,rust: r}
end)
report=%{cases: length(rows),matches: Enum.count(rows,& &1.pass),failures: Enum.filter(rows,&(!&1.pass))};File.write!(Path.join(dir,"report.json"),:json.encode(report));IO.inspect(Map.drop(report,[:failures]));if report.matches != report.cases,do: System.halt(1)
