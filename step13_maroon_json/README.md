# Maroon JSON Executor

This step implements a JSON bytecode executor for the Maroon stack machine.

## Components

1. **JSON Bytecode Format**: Executes the JSON format defined in `maroon_bytecode_schema.json`
2. **Stack Machine**: Implements the same stack-based execution model as the PR
3. **Expression Evaluation**: Supports arithmetic and comparison operations

## Running

```bash
# Build the executor
cd code
cargo build --release

# Run factorial(5) from JSON bytecode
cargo run --release ../../factorial_bytecode.json 5
```

## Stack Machine Operations

- **push_stack**: Push entries onto the stack
- **write**: Output text with variable interpolation
- **sleep**: Delay execution
- **conditional**: Branch based on condition
- **return**: Return value to caller
- **done**: Complete execution

## Example Output

```
0ms: f(5)
250ms: f(4)
450ms: f(3)
600ms: f(2)
700ms: f(1)
750ms: 5!=120
```