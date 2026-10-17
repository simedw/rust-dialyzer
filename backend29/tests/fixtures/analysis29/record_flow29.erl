-module(record_flow29).
-export_record([person]).
-record #person{name = unknown :: atom(), age = 0 :: integer()}.
-import_record(record_dep29, [point]).
-export([create/2, defaults/0, update/2, read/1, pattern/1, guard/1,
         imported/1, imported_read/1, imported_update/2,
         bad_create/0, bad_update/0, bad_native_update/0, bad_imported/0, bad_field/0, bad_pattern/0, bad_guard/0]).
create(Name, Age) -> #person{name = Name, age = Age}.
defaults() -> #person{}.
update(P, Age) -> P#person{age = Age}.
read(P) -> P#person.age.
pattern(#person{age = Age}) -> Age;
pattern(_) -> absent.
guard(P) when is_record(P, person), P#person.age > 0 -> positive;
guard(_) -> other.
imported(X) -> #point{x = X}.
imported_read(P) -> P#point.x.
imported_update(P, X) -> P#point{x = X}.
bad_create() -> #person{age = bad}.
bad_update() -> not_record#person{age = 1}.
bad_native_update() -> (#point{})#person{age = 1}.
bad_imported() -> #point{x = bad}.
bad_field() -> (not_record)#person.age.
bad_pattern() -> case #person{} of #person{age = bad} -> impossible; _ -> ok end.
bad_guard() -> case #person{} of P when P#person.age =:= bad -> impossible; _ -> ok end.
