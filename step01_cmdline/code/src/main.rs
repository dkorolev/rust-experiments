use clap::Parser;

#[derive(Parser)]
struct Args {
  #[arg(long)]
  a: u32,
  #[arg(long)]
  b: u32,
}

fn main() {
  let args = Args::parse();
  println!("{} + {} = {}", args.a, args.b, args.a + args.b);
}
