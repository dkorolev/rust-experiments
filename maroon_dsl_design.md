# Maroon JSON Bytecode Design

A stack machine architecture for executing computational tasks with state management, timing, and output capabilities.

## JSON Bytecode Format

The JSON bytecode directly represents stack machine operations:

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
              "ms": {"mul": [{"var": "n"}, {"const": 50}]},
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
              "condition": {"le": [{"var": "n"}, {"const": 1}]},
              "then": [
                {"type": "return", "value": {"const": 1}}
              ],
              "else": [
                {
                  "type": "push_stack",
                  "entries": [
                    {"Retrn": "FactorialRecursionPostRecursiveCall"},
                    {"Value": {"FactorialArgument": {"sub": [{"var": "n"}, {"const": 1}]}}},
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
  },
  "entry_point": "factorial"
}
```

## Stack Entry Types

The stack machine uses three types of entries:

```rust
enum StackEntry {
  State(String),      // Next state to execute
  Retrn(String),      // Return address
  Value(StackValue),  // Stack values
}

enum StackValue {
  FactorialInput(Expression),
  FactorialArgument(Expression),
  FactorialReturnValue(Expression),
}
```

## Stack Operations

### Push Stack
```json
{
  "type": "push_stack",
  "entries": [
    {"Value": {"FactorialArgument": {"var": "n"}}},
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
  "ms": {"mul": [{"var": "n"}, {"const": 50}]},
  "next_state": "FactorialRecursionPostSleep"
}
```

### Conditional Operation
```json
{
  "type": "conditional",
  "condition": {"le": [{"var": "n"}, {"const": 1}]},
  "then": [{"type": "return", "value": {"const": 1}}],
  "else": [{"type": "push_stack", "entries": [...]}]
}
```

### Return Operation
```json
{
  "type": "return",
  "value": {"mul": [{"var": "n"}, {"var": "result"}]}
}
```

## Expression Evaluation

Expressions in JSON support arithmetic and comparison operations:

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

## VM Implementation

The JSON bytecode is executed by the Rust VM implementation:

**Rust VM** (`step13_maroon_json/`) - High-performance async execution with:
- Complete stack machine implementation
- Expression evaluation for arithmetic and comparisons
- Support for all bytecode operations
- Async/await for sleep operations

## Benefits

1. **Language-agnostic**: JSON bytecode can be executed by any language
2. **Debugging**: Stack state is explicit and inspectable
3. **Performance**: Optimized Rust implementation with async execution
4. **Extensibility**: New operations can be added to the bytecode format
5. **Portability**: JSON format works across platforms and architectures