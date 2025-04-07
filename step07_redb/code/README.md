# Trying Cursor

## Add JSON-parsing Endpoints

Prompt:

```
Add an endpoint, `/sums`.

This endpoint should accept POST-ed JSONs of the following format: `{"id": "foobar", "a": 2, "b": 3, "c": 5}`.

If the schema does not match, the endpoint should return an error.

If the schema does matech but `c` is not `a+b`, the endpoint should return an error too.

Otherwise the endpoint should return OK. Nothing else is required for now.
```
