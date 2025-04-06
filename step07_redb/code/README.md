# Trying Cursor

## Add JSON-parsing Endpoints

Prompt:

```
Add an endpoint, `/sums`.

This endpoint should accept POST-ed JSONs of the following format: `{"id": "foobar", "a": 2, "b": 3, "c": 5}`.

If the schema does not match, the endpoint should return an error.

If the schema does matech but `c` is not `a+b`, the endpoint should return an error too.

Otherwise the endpoint should return OK. Nothing else is required for now.

The JSON from the POST-ed bodies should be parsed manually. This is for just `curl -d ...` work. Axum's standard approach to parsing JSON bodies relies on the `Content-Type` header being set, and we do not need this. Our code should work without the `Content-Type` header being set.
```
