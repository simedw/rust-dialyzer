-module(native_record_dependency29).
-export([point/1]).
-export_record([point]).
-record #point{x=0, y=0}.
point(X) -> #point{x=X}.
