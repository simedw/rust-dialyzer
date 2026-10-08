-module(firefly_call_identity).
-export([local/0, remote/0, local_capture/0, remote_capture/0, closure/0]).
local() -> hidden(1).
remote() -> firefly_call_identity:hidden(2).
local_capture() -> fun hidden/1.
remote_capture() -> fun firefly_call_identity:hidden/1.
closure() -> fun () -> hidden(3) end.
hidden(X) -> X.
