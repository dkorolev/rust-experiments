use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::fs;
use tokio::time::{sleep, Duration};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum StackEntry {
  State { #[serde(rename = "State")] state: String },
  Retrn { #[serde(rename = "Retrn")] retrn: String },
  Value { #[serde(rename = "Value")] value: StackValue },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum StackValue {
  FactorialInput { #[serde(rename = "FactorialInput")] input: Expression },
  FactorialArgument { #[serde(rename = "FactorialArgument")] arg: Expression },
  FactorialReturnValue { #[serde(rename = "FactorialReturnValue")] ret: Expression },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Expression {
  Var { var: String },
  Const { #[serde(rename = "const")] value: u64 },
  Mul { mul: Vec<Expression> },
  Sub { sub: Vec<Expression> },
  Le { le: Vec<Expression> },
}

#[derive(Debug, Serialize, Deserialize)]
struct Operation {
  #[serde(rename = "type")]
  op_type: String,
  #[serde(flatten)]
  data: OperationData,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
enum OperationData {
  PushStack { entries: Vec<StackEntry> },
  Write { text: String, next_state: String },
  Sleep { ms: Expression, next_state: String },
  Return { value: Expression },
  Conditional { condition: Expression, then: Vec<Operation>, #[serde(rename = "else")] else_ops: Vec<Operation> },
  Done {},
}

#[derive(Debug, Serialize, Deserialize)]
struct State {
  name: String,
  local_vars: usize,
  operations: Vec<Operation>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Function {
  states: Vec<State>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Program {
  version: String,
  functions: HashMap<String, Function>,
  entry_point: String,
}

#[derive(Debug)]
struct MaroonVM {
  program: Program,
  stack: Vec<StackEntry>,
  local_vars: HashMap<String, u64>,
  current_time: u64,
}

impl MaroonVM {
  fn new(program: Program) -> Self {
    Self {
      program,
      stack: Vec::new(),
      local_vars: HashMap::new(),
      current_time: 0,
    }
  }

  fn evaluate_expression(&self, expr: &Expression) -> u64 {
    match expr {
      Expression::Var { var } => *self.local_vars.get(var).unwrap_or(&0),
      Expression::Const { value } => *value,
      Expression::Mul { mul } => {
        let a = self.evaluate_expression(&mul[0]);
        let b = self.evaluate_expression(&mul[1]);
        a * b
      }
      Expression::Sub { sub } => {
        let a = self.evaluate_expression(&sub[0]);
        let b = self.evaluate_expression(&sub[1]);
        a - b
      }
      Expression::Le { le } => {
        let a = self.evaluate_expression(&le[0]);
        let b = self.evaluate_expression(&le[1]);
        if a <= b { 1 } else { 0 }
      }
    }
  }

  fn evaluate_stack_value(&self, value: &StackValue) -> u64 {
    match value {
      StackValue::FactorialInput { input } => self.evaluate_expression(input),
      StackValue::FactorialArgument { arg } => self.evaluate_expression(arg),
      StackValue::FactorialReturnValue { ret } => self.evaluate_expression(ret),
    }
  }

  fn interpolate_text(&self, text: &str) -> String {
    let mut result = text.to_string();
    for (var, value) in &self.local_vars {
      result = result.replace(&format!("{{{}}}", var), &value.to_string());
    }
    result
  }

  async fn execute(&mut self, initial_arg: u64) -> Result<()> {
    let entry_func = self.program.functions.get(&self.program.entry_point)
      .context("Entry point function not found")?;

    // Initialize stack with entry state
    self.local_vars.insert("n".to_string(), initial_arg);
    self.stack.push(StackEntry::Value {
      value: StackValue::FactorialInput { input: Expression::Const { value: initial_arg } }
    });
    self.stack.push(StackEntry::State { state: "FactorialEntry".to_string() });

    while !self.stack.is_empty() {
      // Pop current state
      let current_entry = self.stack.pop().unwrap();
      let current_state_name = match current_entry {
        StackEntry::State { state } => state,
        _ => panic!("Expected state on top of stack, got {:?}", current_entry),
      };
      

      // Find the state definition
      let state_def = entry_func.states.iter()
        .find(|s| s.name == current_state_name)
        .context("State not found")?;

      // Pop local variables from stack
      let mut local_values = Vec::new();
      for _ in 0..state_def.local_vars {
        if let Some(StackEntry::Value { value }) = self.stack.pop() {
          local_values.push(self.evaluate_stack_value(&value));
        }
      }
      

      // Update local variables based on state
      if current_state_name.contains("PostRecursiveCall") && local_values.len() >= 2 {
        // local_values[0] is the return value (result), local_values[1] is the saved n
        self.local_vars.insert("result".to_string(), local_values[0]);
        self.local_vars.insert("n".to_string(), local_values[1]);
      } else if (current_state_name == "FactorialRecursiveCall" || 
                 current_state_name.contains("Post") ||
                 current_state_name.contains("Argument")) && !local_values.is_empty() {
        self.local_vars.insert("n".to_string(), local_values[0]);
      }
      

      // Execute operations
      let mut continue_to_next_iteration = false;
      for op in &state_def.operations {
        match &op.op_type[..] {
          "push_stack" => {
            if let OperationData::PushStack { entries } = &op.data {
              for entry in entries {
                self.stack.push(entry.clone());
              }
              // After push_stack in states like FactorialEntry, we should continue 
              // to the next iteration of the main loop
              continue_to_next_iteration = true;
              break;
            }
          }
          "write" => {
            if let OperationData::Write { text, next_state } = &op.data {
              let output = self.interpolate_text(text);
              println!("{}ms: {}", self.current_time, output);
              
              // Push local vars back
              for &val in local_values.iter().rev() {
                self.stack.push(StackEntry::Value {
                  value: StackValue::FactorialArgument { arg: Expression::Const { value: val } }
                });
              }
              self.stack.push(StackEntry::State { state: next_state.clone() });
              break; // Exit the operations loop
            }
          }
          "sleep" => {
            if let OperationData::Sleep { ms, next_state } = &op.data {
              let sleep_ms = self.evaluate_expression(ms);
              sleep(Duration::from_millis(sleep_ms)).await;
              self.current_time += sleep_ms;
              
              // Push local vars back
              for &val in local_values.iter().rev() {
                self.stack.push(StackEntry::Value {
                  value: StackValue::FactorialArgument { arg: Expression::Const { value: val } }
                });
              }
              self.stack.push(StackEntry::State { state: next_state.clone() });
              break; // Exit the operations loop
            }
          }
          "conditional" => {
            if let OperationData::Conditional { condition, then, else_ops } = &op.data {
              let cond_value = self.evaluate_expression(condition);
              let ops_to_execute = if cond_value != 0 { then } else { else_ops };
              
              let mut pushed_stack = false;
              for exec_op in ops_to_execute {
                match &exec_op.op_type[..] {
                  "return" => {
                    if let OperationData::Return { value } = &exec_op.data {
                      let ret_value = self.evaluate_expression(value);
                      
                      // Pop return address
                      if let Some(StackEntry::Retrn { retrn }) = self.stack.pop() {
                        self.stack.push(StackEntry::Value {
                          value: StackValue::FactorialReturnValue { 
                            ret: Expression::Const { value: ret_value }
                          }
                        });
                        self.stack.push(StackEntry::State { state: retrn });
                      }
                    }
                  }
                  "push_stack" => {
                    if let OperationData::PushStack { entries } = &exec_op.data {
                      // First push back the local values to preserve them
                      for &val in local_values.iter().rev() {
                        self.stack.push(StackEntry::Value {
                          value: StackValue::FactorialArgument { arg: Expression::Const { value: val } }
                        });
                      }
                      // Then push the new entries
                      for entry in entries {
                        self.stack.push(entry.clone());
                      }
                      pushed_stack = true;
                    }
                  }
                  _ => {}
                }
              }
              
              if pushed_stack {
                continue_to_next_iteration = true;
                break;
              }
            }
          }
          "return" => {
            if let OperationData::Return { value } = &op.data {
              let ret_value = self.evaluate_expression(value);
              
              // Pop return address
              if let Some(StackEntry::Retrn { retrn }) = self.stack.pop() {
                self.stack.push(StackEntry::Value {
                  value: StackValue::FactorialReturnValue { 
                    ret: Expression::Const { value: ret_value }
                  }
                });
                self.stack.push(StackEntry::State { state: retrn });
              }
            }
          }
          "done" => {
            if local_values.len() >= 2 {
              println!("{}ms: {}!={}", self.current_time, initial_arg, local_values[0]);
            }
            return Ok(());
          }
          _ => {}
        }
      }
      
      // If we set the flag to continue, skip to the next iteration
      if continue_to_next_iteration {
        continue;
      }
    }

    Ok(())
  }
}

#[tokio::main]
async fn main() -> Result<()> {
  let args: Vec<String> = env::args().collect();
  if args.len() != 3 {
    eprintln!("Usage: {} <bytecode.json> <n>", args[0]);
    std::process::exit(1);
  }

  let bytecode_file = &args[1];
  let n: u64 = args[2].parse().context("Invalid number")?;

  let json_content = fs::read_to_string(bytecode_file)
    .context("Failed to read bytecode file")?;

  let program: Program = serde_json::from_str(&json_content)
    .context("Failed to parse bytecode")?;

  let mut vm = MaroonVM::new(program);
  vm.execute(n).await?;

  Ok(())
}