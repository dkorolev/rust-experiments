use maxminddb::Reader;
use std::net::IpAddr;
use std::path::Path;
use trust_dns_resolver::config::{NameServerConfig, Protocol, ResolverConfig, ResolverOpts};
use trust_dns_resolver::TokioAsyncResolver;

const OPENDNS_SERVER: &str = "208.67.222.222:53";
const OPENDNS_MYIP_DOMAIN: &str = "myip.opendns.com.";
const GEOIP_DB_PATH: &str = "GeoLite2-City.mmdb";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  if let Ok(ip) = get_self_ip().await {
    println!("Public IP: {}", ip);
    if let Err(e) = map_ip_to_geo(ip) {
      println!("Error mapping IP to geo: {}", e);
    }
  } else {
    println!("Failed to get your IP address");
  }
  Ok(())
}

async fn get_self_ip() -> Result<IpAddr, Box<dyn std::error::Error>> {
  let mut config = ResolverConfig::new();
  config.add_name_server(NameServerConfig {
    socket_addr: OPENDNS_SERVER.parse()?,
    protocol: Protocol::Udp,
    tls_dns_name: None,
    trust_negative_responses: false,
    bind_addr: None,
  });
  let resolver = TokioAsyncResolver::tokio(config, ResolverOpts::default());
  let response = resolver.lookup_ip(OPENDNS_MYIP_DOMAIN).await?;
  response.iter().next().ok_or_else(|| "No IP found".into())
}

fn map_ip_to_geo(ip: IpAddr) -> Result<(), Box<dyn std::error::Error>> {
  let db_path = Path::new(GEOIP_DB_PATH);
  if !db_path.exists() {
    return Err(format!("GeoIP database file not found: {}", GEOIP_DB_PATH).into());
  }

  let reader = Reader::open_readfile(GEOIP_DB_PATH)?;
  let result: maxminddb::geoip2::City = reader.lookup(ip)?;

  println!("{:#?}", result);

  let iso = result.country.unwrap().iso_code;
  println!("{iso:?}");
  Ok(())
}
