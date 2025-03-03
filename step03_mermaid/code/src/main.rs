mod mermaid {
  use std::fmt;
  use std::cell::RefCell;
  struct Data {
    contents: String,
    i: i32,
  }
  pub struct Canvas {
    instance: RefCell<Data>,
  }
  pub struct Participant<'a> {
    canvas: &'a Canvas,
    i: i32,
    _name: String,
  }
  impl Canvas {
    pub fn new() -> Self {
      Self {
        instance: RefCell::new(Data {
          contents: "sequenceDiagram\n    create participant I0 as root\n    autonumber\n".to_string(),
          i: 0,
        }),
      }
    }

    pub fn new_participant<S: AsRef<str>>(&self, s: S) -> Participant {
      let mut data = self.instance.borrow_mut();
      data.i += 1;
      let i = data.i; // or can just use data.i later on, doesn't matter
      drop(data);
      self.append(format!("    create participant I{} as {}\n    I0-->>I{}: create\n", i, s.as_ref(), i));
      Participant { canvas: self, i, _name: String::from(s.as_ref()) }
    }
    
    fn append<S: AsRef<str>>(&self, s: S) {
      self.instance.borrow_mut().contents.push_str(s.as_ref());
    }
  }

  impl fmt::Display for Canvas {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.instance.borrow().contents)
    }
  }

  impl Participant<'_> {
    pub fn add_arrow_to(&self, rhs: &Participant, text: &str) {
      self.canvas.append(format!("    I{}->>I{}: {}\n", self.i, rhs.i, text));
    }
  }
  impl Drop for Participant<'_> {
    fn drop(&mut self) {
      self.canvas.append(format!("    destroy I{}\n    I{}-->>I0: destroy\n", self.i, self.i));
    }
  }
}

use mermaid::Canvas;
//use medmaid::Participant;

fn main() {
  let canvas = Canvas::new();

  {
    canvas.new_participant("foo");
  }
  {
    let bar = canvas.new_participant("bar");
    {
      let baz = canvas.new_participant("baz");
      bar.add_arrow_to(&baz, "Hello!");
    }
  }

  {
    let meh = canvas.new_participant("meh");
    {
      let blah = canvas.new_participant("blah");
      blah.add_arrow_to(&meh, "Whoa!");
    }
  }

  println!("{}", canvas);
}
