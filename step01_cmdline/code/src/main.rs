use clap::Parser;

#[derive(Parser, clap::ValueEnum, Clone, Debug)]
enum Operation {
  Add,
  Mul,
}

#[derive(Parser)]
struct Args {
  #[arg(long)]
  a: i8,
  #[arg(long)]
  b: i8,
  #[arg(long, value_enum, default_value_t = Operation::Add)]
  op: Operation,
}

fn main() {
  let args = Args::parse();
  let (result, op_symbol) = match args.op {
    Operation::Add => (args.a + args.b, '+'),
    Operation::Mul => (args.a * args.b, '*'),
  };
  println!("{} {} {} = {}", args.a, op_symbol, args.b, result);
}