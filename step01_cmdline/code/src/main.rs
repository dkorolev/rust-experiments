use clap::Parser;

#[derive(Parser)]
struct Args {
  #[arg(long)]
  a: i8,
  #[arg(long)]
  b: i8,
}

fn main() {
  let args = Args::parse();
  println!("{} + {} = {}", args.a, args.b, args.a + args.b);
}
