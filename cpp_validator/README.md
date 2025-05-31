# C++ Maroon Validator

This is a C++ implementation of the Maroon stack machine that validates execution against the Rust implementation.

## Dependencies

- C++17 compiler
- nlohmann/json library

On macOS with Homebrew:
```bash
brew install nlohmann-json
```

On Ubuntu/Debian:
```bash
sudo apt-get install nlohmann-json3-dev
```

## Building

```bash
make
```

## Running

```bash
./maroon_validator ../factorial_bytecode.json 5
```

## Implementation

The C++ validator implements:
- Full stack machine execution model
- Expression evaluation (var, const, mul, sub, le)
- All operations: push_stack, write, sleep, conditional, return, done
- Exact same output format as Rust implementation

## Validation

Compare output between Rust and C++ implementations:
```bash
# Rust
cd ../step13_maroon_json/code
cargo run --release ../../factorial_bytecode.json 5 > rust_output.txt

# C++
cd ../../cpp_validator
./maroon_validator ../factorial_bytecode.json 5 > cpp_output.txt

# Compare
diff rust_output.txt cpp_output.txt
```

Both should produce identical output, validating the correctness of the JSON bytecode execution model.