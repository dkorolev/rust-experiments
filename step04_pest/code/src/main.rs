use pest::Parser;
use pest::iterators::Pair;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "grammar.pest"]
struct Grammar;

fn traverse(token: Pair<'_, Rule>, indent: usize) {
  println!("{}{:?} {}", " ".repeat(indent), token.as_rule(), token.as_str());
  token.into_inner().for_each(|e| traverse(e, indent + 2));
}

fn main() {
  let s = "i,J,k1,Q1,t_7,z_9,foo_bar,FOO_BAR,a_b_1,C_D_4,e_f5,G_H6,UPPER";
  Grammar::parse(Rule::terms, s).unwrap().for_each(|e| traverse(e, 0));
}

#[cfg(test)]
mod tests {
  use super::*;

  fn rule(s: &'static str) -> Option<Rule> {
    match Grammar::parse(Rule::term, s) {
      Ok(mut r) => Some(r.next().unwrap().as_rule()),
      Err(_) => None,
    }
  }

  #[test]
  fn test() {
    assert_eq!(rule("test"), Some(Rule::lower_snake));
    assert_eq!(rule("foo_bar"), Some(Rule::lower_snake));
    assert_eq!(rule("i"), Some(Rule::lower_snake));
    assert_eq!(rule("i1"), Some(Rule::lower_snake));
    assert_eq!(rule("i_1"), Some(Rule::lower_snake));

    assert_eq!(rule("TEST"), Some(Rule::upper_snake));
    assert_eq!(rule("FOO_BAR"), Some(Rule::upper_snake));
    assert_eq!(rule("FOO_BAR_BAZ"), Some(Rule::upper_snake));
    assert_eq!(rule("I"), Some(Rule::upper_snake));
    assert_eq!(rule("I1"), Some(Rule::upper_snake));
    assert_eq!(rule("I_1"), Some(Rule::upper_snake));

    assert_eq!(rule("Test"), Some(Rule::camel_case));
    assert_eq!(rule("FooBar"), Some(Rule::camel_case));
    assert_eq!(rule("FooBarBaz"), Some(Rule::camel_case));
    assert_eq!(rule("FooBar1"), Some(Rule::camel_case));

    assert_eq!(rule("fooBar"), Some(Rule::camel_back));
    assert_eq!(rule("fooBarBaz"), Some(Rule::camel_back));
    assert_eq!(rule("fooBar1"), Some(Rule::camel_back));

    assert_eq!(rule("FooBar_1"), None);
    assert_eq!(rule("mixedCASE"), None);
    assert_eq!(rule("FAILStoo"), None);
    assert_eq!(rule("1foo"), None);
    assert_eq!(rule("1Foo"), None);
    assert_eq!(rule("fooBar_1"), None);
    assert_eq!(rule("fooBarBAZ"), None);
    assert_eq!(rule("fooBar_Baz"), None);
    assert_eq!(rule("_foo"), None);
    assert_eq!(rule("_FOO"), None);
    assert_eq!(rule("_Foo"), None);
  }
}
