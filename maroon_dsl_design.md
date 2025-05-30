# Maroon DSL Design

Based on the PR's stack machine architecture with `MaroonTaskStackEntry`:

## DSL Example (Factorial)

```maroon
function factorial(n: u64) -> u64 {
  write("f({n})")
  sleep(n * 50)
  
  if n <= 1 {
    return 1
  } else {
    let result = factorial(n - 1)
    return n * result
  }
}
```

## JSON Bytecode (Direct mapping to MaroonTaskStackEntry)

The JSON directly represents the stack machine operations from the PR:

```json
{
  "version": "1.0",
  "functions": {
    "factorial": {
      "states": [
        {
          "name": "FactorialEntry",
          "local_vars": 1,
          "operations": [
            {
              "type": "push_stack",
              "entries": [
                {"Value": {"FactorialInput": {"var": "n"}}},
                {"Retrn": "FactorialDone"},
                {"Value": {"FactorialArgument": {"var": "n"}}},
                {"State": "FactorialRecursiveCall"}
              ]
            }
          ]
        },
        {
          "name": "FactorialRecursiveCall",
          "local_vars": 1,
          "operations": [
            {
              "type": "write",
              "text": "f({n})",
              "next_state": "FactorialRecursionPostWrite"
            }
          ]
        },
        {
          "name": "FactorialRecursionPostWrite",
          "local_vars": 1,
          "operations": [
            {
              "type": "sleep",
              "ms": {"mul": [{"var": "n"}, 50]},
              "next_state": "FactorialRecursionPostSleep"
            }
          ]
        },
        {
          "name": "FactorialRecursionPostSleep",
          "local_vars": 1,
          "operations": [
            {
              "type": "conditional",
              "condition": {"le": [{"var": "n"}, 1]},
              "then": [
                {"type": "return", "value": {"const": 1}}
              ],
              "else": [
                {
                  "type": "push_stack",
                  "entries": [
                    {"Retrn": "FactorialRecursionPostRecursiveCall"},
                    {"Value": {"FactorialArgument": {"sub": [{"var": "n"}, 1]}}},
                    {"State": "FactorialRecursiveCall"}
                  ]
                }
              ]
            }
          ]
        },
        {
          "name": "FactorialRecursionPostRecursiveCall",
          "local_vars": 2,
          "operations": [
            {
              "type": "return",
              "value": {"mul": [{"var": "n"}, {"var": "result"}]}
            }
          ]
        },
        {
          "name": "FactorialDone",
          "local_vars": 2,
          "operations": [
            {
              "type": "done"
            }
          ]
        }
      ]
    }
  }
}
```

## Stack Entry Types (from PR)

```rust
enum MaroonTaskStackEntry {
  State(MaroonTaskState),      // Next state to execute
  Retrn(MaroonTaskState),      // Return address
  Value(MaroonTaskStackEntryValue), // Stack values
}

enum MaroonTaskStackEntryValue {
  FactorialInput(u64),
  FactorialArgument(u64),
  FactorialReturnValue(u64),
}
```

## JSON Stack Operations

### Push Stack
```json
{
  "type": "push_stack",
  "entries": [
    {"Value": {"FactorialArgument": 5}},
    {"State": "FactorialRecursiveCall"}
  ]
}
```

### Write Operation
```json
{
  "type": "write",
  "text": "f({n})",
  "next_state": "FactorialRecursionPostWrite"
}
```

### Sleep Operation
```json
{
  "type": "sleep",
  "ms": {"mul": [{"var": "n"}, 50]},
  "next_state": "FactorialRecursionPostSleep"
}
```

### Return Operation
```json
{
  "type": "return",
  "value": {"FactorialReturnValue": {"mul": [{"var": "n"}, {"var": "result"}]}}
}
```

## Expression Evaluation

Expressions in JSON map to stack machine computations:

```json
// Variable reference
{"var": "n"}

// Constant
{"const": 1}

// Arithmetic
{"mul": [{"var": "n"}, {"const": 50}]}
{"sub": [{"var": "n"}, {"const": 1}]}

// Comparison
{"le": [{"var": "n"}, {"const": 1}]}
```

## Benefits of this approach:

1. **Direct mapping**: JSON maps 1:1 to the PR's `MaroonTaskStackEntry` types
2. **Multi-language**: Same JSON can be executed by Rust, C++, or any runtime
3. **Validation**: C++ node can validate execution matches Rust exactly
4. **Debugging**: Stack state is explicit and inspectable
5. **Performance**: Can compare execution across implementations

The DSL compiler would:
1. Parse Maroon DSL syntax
2. Generate state machine states
3. Output JSON matching the stack machine model
4. JSON becomes the "bytecode" for execution