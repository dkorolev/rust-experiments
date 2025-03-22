use clap::Parser;
use redis::AsyncCommands;
use std::future::Future;
use std::process::ExitCode;

#[derive(Parser)]
struct Args {
  #[arg(long, default_value = "redis://127.0.0.1")]
  redis: String,

  #[arg(long)]
  mode: String,
}

async fn run_check(args: &Args) -> Result<u8, redis::RedisError> {
  let _ = redis::Client::open(args.redis.to_string())?.get_multiplexed_async_connection().await?;
  println!("Redis is available.");
  Ok(0)
}

async fn run_test(args: &Args) -> Result<u8, redis::RedisError> {
  let client = redis::Client::open(args.redis.to_string())?;
  let mut con = client.get_multiplexed_async_connection().await?;

  let _: () = con.set("key", "value").await.expect("failed");
  let val: String = con.get("key").await?;

  println!("Got value: {}", val);

  if val == "value" { Ok(0) } else { Ok(1) }
}

async fn run_arm<'a, F, RETVAL>(f: F, args: &'a Args) -> ExitCode
where
  F: FnOnce(&'a Args) -> RETVAL,
  RETVAL: Future<Output = Result<u8, redis::RedisError>>,
{
  let result = f(&args).await;
  ExitCode::from(match result {
    Ok(code) => code,
    _ => {
      println!("Something failed, or Redis is not available.");
      1
    }
  })
}

#[tokio::main]
async fn main() -> ExitCode {
  let args = Args::parse();
  match args.mode.as_str() {
    "check" => run_arm(&run_check, &args).await,
    "test" => run_arm(&run_test, &args).await,
    _ => {
      println!("This `--mode` value is not supported.");
      ExitCode::from(1)
    }
  }
}
