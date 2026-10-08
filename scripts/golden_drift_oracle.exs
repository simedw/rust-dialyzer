[root]=System.argv()
suite=Path.join(root,".cache/upstream/otp/lib/dialyzer/test/opaque_SUITE_data")
proof=Path.join(root,".cache/golden_drift")
File.mkdir_p!(Path.join(proof,"suite/src/ewgi"));File.mkdir_p!(Path.join(proof,"suite/results"));File.mkdir_p!(Path.join(proof,"out"))
for path<-Path.wildcard(Path.join(suite,"src/ewgi/*")) do
 bytes=File.read!(path)
 bytes=if Path.extname(path)==".erl" do
  [before,after_text]=String.split(bytes,"%% Copyright Ericsson AB 2025. All Rights Reserved.\n")
  before<>after_text
 else bytes end
 File.write!(Path.join(proof,"suite/src/ewgi/"<>Path.basename(path)),bytes)
end
File.cp!(Path.join(suite,"results/ewgi"),Path.join(proof,"suite/results/ewgi"))
base=Path.join(root,".cache/corpus/otp/base.plt")
link=Path.join(proof,"out/dialyzer_plt")
if not File.exists?(link),do: File.ln_s!(base,link)
true=:code.add_patha(String.to_charlist(Path.join(root,".cache/corpus/otp/helpers")))
opts=[indent_opt: false,warnings: [:no_unused,:no_return,:no_unknown,:opaque_union]]
original=:dialyzer_common.check(:ewgi,opts,String.to_charlist(suite),String.to_charlist(Path.join(proof,"out")))
shifted=:dialyzer_common.check(:ewgi,opts,String.to_charlist(Path.join(proof,"suite")),String.to_charlist(Path.join(proof,"out")))
true=match?({:differ,:ewgi,_},original)
:same=shifted
IO.puts(:json.encode(%{original_golden: "differ",single_header_line_removed: "same",differing_warning_lines: length(elem(original,2)) |> div(2),scope: "causal source-line reproduction; pinned originals unchanged"}))
