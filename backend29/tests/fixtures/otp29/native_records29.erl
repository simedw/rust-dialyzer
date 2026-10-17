-module(native_records29).
-export([create/2, defaults/0, update/2, update_external/2, update_any/2,
         read/1, read_external/1, read_any/1, pattern/1, guard/1,
         imported/1, imported_read/1, imported_update/2, predicates/1,
         create_calls/1, update_calls/2, same_fields/1, nested_binary/1]).
-export_record([person]).
-record #person{name=unknown :: atom(), age=0 :: integer(), meta}.
-import_record(native_record_dependency29, [point]).

create(Name, Meta) -> #person{name=Name, meta=Meta}.
defaults() -> #person{meta=none}.
update(P, Age) -> P#person{age=Age}.
update_external(P, Age) -> P#native_records29:person{age=Age}.
update_any(P, Age) -> P#_{age=Age}.
read(P) -> P#person.age.
read_external(P) -> P#native_records29:person.age.
read_any(P) -> P#_.age.
pattern(#person{name=N, age=A}) -> {N,A};
pattern(#_{}) -> any.
guard(P) when is_record(P, person), P#person.age > 0 -> yes;
guard(_) -> no.
imported(X) -> #point{x=X}.
imported_read(P) -> P#point.x.
imported_update(P, X) -> P#point{x=X}.
predicates(X) when is_integer(X, 0, 10) -> integer;
predicates(X) -> is_record(X).
create_calls(X) -> #person{name=id(X), age=id(1), meta=id(X)}.
update_calls(P, X) -> P#person{name=id(X), meta=id(X)}.
same_fields(#person{name=X, meta=X}) -> X;
same_fields(_) -> different.
nested_binary(#person{meta = <<N:8, Rest:N/binary>>}) -> Rest;
nested_binary(_) -> no.
id(X) -> X.
