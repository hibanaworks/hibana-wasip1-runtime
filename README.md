# hibana-wasip1-runtime

`hibana-wasip1-runtime` is the Hibana-bound WASI Preview 1 runtime layer.

It owns the guest engine, WASI P1 import payload protocol, and ChoreoFS driver
facts used to complete guest imports only through Hibana Endpoint/carrier
progress. It is not a general WASI host, filesystem fallback, socket runtime,
or component-model engine.
