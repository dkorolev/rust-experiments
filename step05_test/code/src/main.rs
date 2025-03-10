use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct Grammar;

fn traverse(token: Pair<'_, Rule>, indent: usize) {
  let (next_indent, proceed) = match token.as_rule() {
    // These next two lines are unnecessary, since the terms are declared as `_{ ... }`, where `_` means "silent".
    // Rule::toplevel => (0, true),
    // Rule::term => (0, true),
    Rule::identifier => {
      println!("{}S: {}", " ".repeat(indent), token.as_str());
      (2, true)
    }
    Rule::number => {
      println!("{}N: {}", " ".repeat(indent), token.as_str());
      (0, false)
    }
    Rule::op => {
      println!("{}V: {}", " ".repeat(indent), token.as_str());
      (0, false)
    }
    _ => {
      println!("{}INTERNAL {:?} {}", " ".repeat(indent), token.as_rule(), token.as_str());
      (2, true)
    }
  };
  if proceed {
    token.into_inner().for_each(|e| traverse(e, next_indent));
  }
}

fn main() {
  Grammar::parse(Rule::toplevel, "a + b a2 - b2 + 42").unwrap().for_each(|e| traverse(e, 0));
}
