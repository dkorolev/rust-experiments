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

## Keep `id`-s Unique

Prompt:

```
Make sure to store the `.id`-s of the POST-ed payloads to `/sums` in the database.

Process requests with new `.id`-s same way as before, just store those IDs.

When a request comes with a duplicate `.id`, disregard it with the respective message.

Also, the `.id`-s should be non-empty strings.
```

## Track and Return Recent Requests

```
Now, let's make sure some one thousand (configurable with a constant) most recent requests to `/sums` are journaled.

Add a GET endpoint to return the most recent N, for a provided N, up to this configured constant number of them that are stored.
```
