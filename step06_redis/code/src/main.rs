use clap::Parser;
use redis::AsyncCommands;
use std::process::ExitCode;
use anyhow::Result;
use anyhow::anyhow;

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "redis://127.0.0.1")]
  redis: String,

  #[arg(long)]
  mode: String,
}

async fn try_check(args: &Args) -> Result<()> {
  let _ = redis::Client::open(args.redis.to_string())?.get_multiplexed_async_connection().await?;
  println!("Redis is available.");
  Ok(())
}

async fn try_test(args: &Args) -> Result<()> {
  let client = redis::Client::open(args.redis.to_string())?;
  let mut con = client.get_multiplexed_async_connection().await?;

  let _: () = con.set("key", "value").await?;
  let val: String = con.get("key").await?;

  println!("Got value: {}", val);

  (val == "value").then(|| ()).ok_or(anyhow!("Wrong value"))
}

#[tokio::main]
async fn main() -> ExitCode {
  let args = Args::parse();

  match args.mode.as_str() {
    "check" => try_check(&args).await.map_or(1, |_| 0),
    "test" => try_test(&args).await.map_or(1, |_| 0),
    _ => {
      println!("This `--mode` value is not supported.");
      1
    }
  }.into()
}
