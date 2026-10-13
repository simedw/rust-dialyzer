# Records what Elixir's inline-pure functions return for literal arguments,
# for tests/elixir_pure_oracle.rs. Run with Elixir 1.19.4 on OTP 28.3:
#
#   elixir scripts/elixir_pure_oracle.exs tests/fixtures/elixir_pure_cases.json
#
# A result of "raises" means the compiler keeps the call.
[output] = System.argv()
:rand.seed(:exsss, {19, 4, 28})

defmodule Oracle do
  def encode(t) when is_atom(t), do: %{"a" => Atom.to_string(t)}
  def encode(t) when is_integer(t), do: %{"i" => Integer.to_string(t)}
  def encode(t) when is_binary(t), do: %{"b" => Base.encode16(t, case: :lower)}
  def encode(t) when is_list(t), do: %{"l" => Enum.map(t, &encode/1)}
  def encode(t) when is_tuple(t), do: %{"t" => t |> Tuple.to_list() |> Enum.map(&encode/1)}

  def encode(t) when is_map(t) do
    pairs = for {k, v} <- Map.to_list(t), do: [encode(k), encode(v)]
    %{"m" => Enum.sort_by(pairs, fn [k, _] -> :json.encode(k) |> IO.iodata_to_binary() end)}
  end

  def run(module, fun, args) do
    result =
      try do
        {:ok, apply(module, fun, args)}
      rescue
        _ -> "raises"
      catch
        _, _ -> "raises"
      end

    result = with {:ok, value} <- result, do: encode(value)

    %{"module" => Atom.to_string(module), "function" => Atom.to_string(fun),
      "args" => Enum.map(args, &encode/1), "result" => result}
  end

  def pick(list), do: Enum.at(list, :rand.uniform(length(list)) - 1)
  def string(alphabet, max), do: for(_ <- 1..:rand.uniform(max + 1) - 1//1, into: "", do: pick(alphabet))
end

run = &Oracle.run/3
pick = &Oracle.pick/1
string = &Oracle.string/2

uri_chars =
  ~w(a b z A Z 0 1 9 : / ? # [ ] @ ! $ & ' \( \) * + , ; = % - . _ ~) ++
    [" ", "\n", "\r", "é", "€", <<0xFF>>, <<0xC3>>, "%2", "%41", "%zz", "//", "::", "[::1]"]

uri_parts = fn ->
  scheme = pick.(["", "", "http:", "HTTPS:", "x+y-1.:", "ws:", "ftp:", "1a:", "mailto:"])
  userinfo = pick.(["", "", "user@", "u:p@", "@", "a@b@", "u%20@"])
  host =
    pick.(["example.com", "", "127.0.0.1", "1.2.3.400", "01.2.3.4", "[::1]", "[2001:db8::7]",
          "[::ffff:1.2.3.4]", "[1:2:3:4:5:6:7:8]", "[1:2:3:4:5:6:7:8:9]", "[1::2::3]", "[::]",
          "[v1.x]", "[fe80::1%25en0]", "[", "[::1", "[]", "h_o-s.t~", "é.com", "a b"])
  port = pick.(["", "", ":80", ":", ":0443", ":99999999999999999999", ":8a"])
  authority = pick.(["", "//" <> userinfo <> host <> port, "//" <> userinfo <> host <> port, "//"])
  path = pick.(["", "/", "/a/b", "a/b", "/a:b", "a:b", "//x", "/%7e", "/é", "/ a"])
  query = pick.(["", "", "?", "?a=1&b=2", "?a?b/c", "?#", "?a b"])
  fragment = pick.(["", "", "#", "#f", "#a/b?c", "#a#b", "#x\ny"])
  scheme <> authority <> path <> query <> fragment
end

ipv6 = fn ->
  groups = for _ <- 1..:rand.uniform(9) - 1//1, do: pick.(["0", "1", "ab", "fFfF", "1234", "12345", "00000"])
  {left, right} = Enum.split(groups, :rand.uniform(length(groups) + 1) - 1)
  sep = pick.([":", "::", ":", "::"])
  tail = pick.(["", "", ":1.2.3.4", "1.2.3.4", ":256.1.1.1", ":01.2.3.4", ":1.2.3"])
  Enum.join(left, ":") <> sep <> Enum.join(right, ":") <> tail
end

uris =
  ["", "https://example.com/temp", "http://user:pw@host:8080/p?q=1#f", "HTTP://EXAMPLE.COM",
   "//host", "///", "mailto:a@b.c", "urn:isbn:0451450523", "foo", "/rel/path", "?q", "#f",
   "http://[::1]:4000/x", "http://[::1", "http://a@b@c", "http://host:abc", "http://h:/p",
   "file:///etc/hosts", "http://1.2.3.4:5/", "a:", ":", "http://\n", <<0xFF>>, "http://é.com"] ++
    for(_ <- 1..600, do: uri_parts.()) ++ for(_ <- 1..600, do: string.(uri_chars, 24)) ++
    for(_ <- 1..600, do: "http://[" <> string.(~w(0 1 f F 255 ffff 1234 12345 : :: . 1.2.3.4 0.0 g), 12) <> pick.(["]", "]:8", "]/x", "]x", ""])) ++
    for(_ <- 1..600, do: "//[" <> ipv6.() <> "]")

cases =
  for uri <- uris, fun <- [:parse, :new, :new!] do
    run.(URI, fun, [uri])
  end

text_chars = ~w(a Z 0 - . _ ~ + % %2 %41 %4a %zz) ++ [" ", "&", "=", "/", "é", <<0xFF>>, "\n"]
texts = ["", "a b", "a+b", "100%", "%%", "%E2%82%AC"] ++ for(_ <- 1..400, do: string.(text_chars, 12))

cases =
  cases ++
    for(t <- texts, fun <- [:encode_www_form, :decode], do: run.(URI, fun, [t])) ++
    for(
      args <- [
        [[]], [[], :bogus], [[], "x"], [[a: 1, b: "x y"]], [[{"k", "v&w"}, {:nil, nil}]],
        [[a: true, b: false, c: :atom]], [[{1, 2}]], [[a: [1]]], [[{[1], 2}]], [[{:a, {1, 2}}]],
        [[a: "é ~"], :rfc3986], [[a: "é ~"], :www_form], [[a: 1], :other], [[:a]], [[{:a, 1, 2}]],
        [:atom], ["string"], [[a: "é"]]
      ],
      do: run.(URI, :encode_query, args)
    ) ++
    for(arg <- [1, :a, ["x"], {"a", "b"}], fun <- [:parse, :new, :new!, :decode, :encode_www_form],
        do: run.(URI, fun, [arg])) ++
    [run.(URI, :decode_www_for, ["a+b"])]

version_chars = ~w(0 1 2 9 . - + a Z) ++ ["", "00", "10"]
versions =
  ["1.2.3", "1.2", "1.2.3-rc.1", "1.2.3+build.5", "1.2.3-rc.01", "1.2.3-0.a", "01.2.3",
   "1.2.3-", "1.2.3+", "1.2.3.4", "1.2.3-a..b", "1.2.3+b+c", "1.2.3-x-y+z-w", "", "é"] ++
    for(_ <- 1..600, do: string.(version_chars, 10))

ops = ["", "== ", ">= ", "<= ", "> ", "< ", "~> ", "!= ", "!", ">=", "~>"]
requirement = fn ->
  clause = fn -> pick.(ops) <> pick.(["1.2.3", "1.2", "1.0.0-rc", "2.0", "a", "1.2.3+b"]) end
  Enum.map_join(1..:rand.uniform(3), pick.([" and ", " or ", " and ", "  ", " ", " xor "]), fn _ -> clause.() end)
end

requirements =
  ["~> 1.2", "== 1.2.3", ">= 1.0.0 and < 2.0.0", "1.2.3", "1.2", "~> 1.2 or ~> 2.0", "",
   " and ", "and 1.0.0", "!= 1.0.0", ">= 1.0.0 and", "1.0.0 2.0.0", <<0xFF>>, "é"] ++
    for(_ <- 1..500, do: requirement.())

cases =
  cases ++
    for(v <- versions, fun <- [:parse, :parse!], do: run.(Version, fun, [v])) ++
    for(r <- requirements, fun <- [:parse_requirement, :parse_requirement!],
        do: run.(Version, fun, [r])) ++
    for(arg <- [1, :a], fun <- [:parse, :parse_requirement], do: run.(Version, fun, [arg]))

durations = [
  [], [day: 1], [year: 1, month: 2, week: 3, day: 4, hour: 5, minute: 6, second: 7],
  [microsecond: {10, 3}], [microsecond: {10, 7}], [microsecond: 10], [day: 1, day: 2],
  [days: 1], [day: "1"], [{"day", 1}], [:day], :day, [day: -1], [microsecond: {-5, 0}]
]

cases = cases ++ for(d <- durations, do: run.(Duration, :new!, [d]))

split_chars = ["a", "b", ",", ", ", " ", "\t", "\n", "\u00A0", "\u2003", "\u3000", "é", "e\u0301", <<0xFF>>, ""]
strings = ["", " ", "a b  c ", "a,b,,c,", ",", "abc"] ++ for(_ <- 1..300, do: string.(split_chars, 10))
patterns = [",", ", ", " ", "", [], [",", " "], ["a", "ab"], [""], [:x], "b"]
options = [
  [], [trim: true], [trim: false], [parts: 1], [parts: 2], [parts: 3], [parts: 2, trim: true],
  [parts: 3, trim: true], [parts: :infinity, trim: true], [parts: 0], [trim: :yes],
  [parts: 2, trim: :yes], [{:trim}, :x], [{:trim, true, 1}], [:x, {:trim, true}]
]

cases =
  cases ++
    for(s <- strings, do: run.(String, :split, [s])) ++
    for(s <- Enum.take(strings, 120), p <- patterns, do: run.(String, :split, [s, p])) ++
    for(s <- Enum.take(strings, 30), p <- patterns, o <- options, do: run.(String, :split, [s, p, o])) ++
    [run.(String, :split, [:a]), run.(String, :split, ["a", ",", :bad])]

File.write!(output, :json.encode(cases))
IO.puts("#{length(cases)} cases")
