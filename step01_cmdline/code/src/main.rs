use clap::Parser;

#[derive(Parser)]
struct Args {
  #[arg(long)]
  a: u8,
  #[arg(long)]
  b: u8,
}

fn main() {
  let args = Args::parse();
  println!("{} + {} = {}", args.a, args.b, args.a + args.b);
}
