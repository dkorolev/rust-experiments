# Trying Cursor

## Add JSON-parsing Endpoints

Prompt:

```
Add an endpoint, `/sums`.

This endpoint should accept POST-ed JSONs of the following format: `{"id": "foobar", "a": 2, "b": 3, "c": 5}`.

Do not use standard Axum ways to parse the JSON. You should accept any `Content-Type`. Parse the JSON into the respective type. Do not analyze individual fields of the JSON object! Use the standard tool such as `serde`.

If the JSON schema does not match, the endpoint should return an error.

If the JSON schema does matech but `c` is not `a+b`, the endpoint should return an error too.

Otherwise the endpoint should return OK. Nothing else is required for now.
```
