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

  fn rule(s: &'static str) -> Rule {
    Grammar::parse(Rule::term, s).unwrap().next().unwrap().as_rule()
  }

  #[test]
  fn test_good() {
    assert_eq!(rule("test"), Rule::lower_snake);
    assert_eq!(rule("foo_bar"), Rule::lower_snake);
    assert_eq!(rule("i"), Rule::lower_snake);
    assert_eq!(rule("i1"), Rule::lower_snake);
    assert_eq!(rule("i_1"), Rule::lower_snake);

    assert_eq!(rule("TEST"), Rule::upper_snake);
    assert_eq!(rule("FOO_BAR"), Rule::upper_snake);
    assert_eq!(rule("FOO_BAR_BAZ"), Rule::upper_snake);
    assert_eq!(rule("I"), Rule::upper_snake);
    assert_eq!(rule("I1"), Rule::upper_snake);
    assert_eq!(rule("I_1"), Rule::upper_snake);

    assert_eq!(rule("Test"), Rule::camel_case);
    assert_eq!(rule("FooBar"), Rule::camel_case);
    assert_eq!(rule("FooBarBaz"), Rule::camel_case);
    assert_eq!(rule("FooBar1"), Rule::camel_case);

    assert_eq!(rule("fooBar"), Rule::camel_back);
    assert_eq!(rule("fooBarBaz"), Rule::camel_back);
    assert_eq!(rule("fooBar1"), Rule::camel_back);
  }

  #[test]
  #[should_panic]
  fn test_panic_01() {
    rule("FooBar_1");
  }

  #[test]
  #[should_panic]
  fn test_panic_02() {
    rule("mixedCASE");
  }

  #[test]
  #[should_panic]
  fn test_panic_03() {
    rule("FAILStoo");
  }

  #[test]
  #[should_panic]
  fn test_panic_04() {
    rule("1foo");
  }

  #[test]
  #[should_panic]
  fn test_panic_05() {
    rule("1Foo");
  }

  #[test]
  #[should_panic]
  fn test_panic_06() {
    rule("fooBar_1");
  }

  #[test]
  #[should_panic]
  fn test_panic_07() {
    rule("fooBarBAZ");
  }

  #[test]
  #[should_panic]
  fn test_panic_08() {
    rule("fooBar_Baz");
  }

  #[test]
  #[should_panic]
  fn test_panic_09() {
    rule("_foo");
  }

  #[test]
  #[should_panic]
  fn test_panic_10() {
    rule("_FOO");
  }

  #[test]
  #[should_panic]
  fn test_panic_11() {
    rule("_Foo");
  }
}
