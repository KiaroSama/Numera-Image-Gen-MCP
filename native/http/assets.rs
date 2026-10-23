use crate::compat::value::{JsString, Value};
use crate::error::{NumeraError, Result};
use crate::http::bounded::BoundedBytes;
use std::{
    future::Future,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use url::Url;

pub fn private_address(address: &str) -> bool {
    let Ok(ip) = address.parse::<IpAddr>() else {
        return true;
    };
    match ip {
        IpAddr::V4(ip) => private_v4(ip),
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return private_v4(v4);
            }
            let bytes = ip.octets();
            ip.is_unspecified()
                || ip.is_loopback()
                || bytes[0] & 0xfe == 0xfc
                || (bytes[0] == 0xfe && bytes[1] & 0xc0 == 0x80)
                || bytes[0] == 0xff
                || bytes[..4] == [0x20, 0x01, 0x0d, 0xb8]
        }
    }
}
fn private_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    a == 0
        || a == 10
        || (a == 100 && (64..128).contains(&b))
        || a == 127
        || (a == 169 && b == 254)
        || (a == 172 && (16..32).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 192 && b == 168)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 224
}

pub async fn fetch(
    raw: &JsString,
    config: &Value,
    limit: usize,
    cancel: &CancellationToken,
) -> Result<Vec<u8>> {
    fetch_with_resolver(raw, config, limit, cancel, |host, port| async move {
        tokio::net::lookup_host((host.as_str(), port))
            .await
            .map(|addresses| addresses.map(|address| address.ip()).collect())
    })
    .await
}

/// Supplies hostname resolution only; destination validation and socket pinning remain shared.
pub async fn fetch_with_resolver<F, A>(
    raw: &JsString,
    config: &Value,
    limit: usize,
    cancel: &CancellationToken,
    resolve: F,
) -> Result<Vec<u8>>
where
    F: Fn(String, u16) -> A + Send + Sync,
    A: Future<Output = std::io::Result<Vec<IpAddr>>> + Send,
{
    let operation = async {
        let mut url = Url::parse(&String::from_utf16_lossy(raw.as_utf16()))
            .map_err(|_| NumeraError::new("unsafe_destination", "Invalid image URL."))?;
        for _ in 0..4 {
            if !matches!(url.scheme(), "http" | "https")
                || !url.username().is_empty()
                || url.password().is_some()
                || url.fragment().is_some()
            {
                return Err(NumeraError::new("unsafe_destination", "Unsafe image URL."));
            }
            let origin = url.origin().ascii_serialization();
            let allowance = config["files"]["assetAllowances"]
                .as_array()
                .is_some_and(|items| {
                    items.iter().any(|a| {
                        let Some(prefix) = a["pathPrefix"].as_str() else {
                            return false;
                        };
                        let Some(origin_units) = a["origin"].as_utf16() else {
                            return false;
                        };
                        Url::parse(&String::from_utf16_lossy(origin_units))
                            .is_ok_and(|base| base.origin().ascii_serialization() == origin)
                            && (url.path() == prefix
                                || url
                                    .path()
                                    .starts_with(&format!("{}/", prefix.trim_end_matches('/'))))
                    })
                });
            let host = url
                .host_str()
                .ok_or_else(|| NumeraError::new("unsafe_destination", "Unsafe image URL."))?
                .trim_matches(['[', ']'])
                .to_owned();
            let port = url
                .port_or_known_default()
                .ok_or_else(NumeraError::runtime)?;
            let addresses: Vec<SocketAddr> = if let Ok(ip) = host.parse::<IpAddr>() {
                vec![SocketAddr::new(ip, port)]
            } else {
                resolve(host.clone(), port)
                    .await
                    .map_err(|_| {
                        NumeraError::new(
                            "input_file_error",
                            "Image destination cannot be resolved.",
                        )
                        .stage("download")
                    })?
                    .into_iter()
                    .map(|ip| SocketAddr::new(ip, port))
                    .collect()
            };
            if addresses.is_empty()
                || (!allowance
                    && addresses
                        .iter()
                        .any(|a| private_address(&a.ip().to_string())))
            {
                return Err(NumeraError::new(
                    "unsafe_destination",
                    "Image URL resolves to a private, reserved or metadata destination.",
                ));
            }
            let first = [addresses[0]];
            let client = super::api::client(Duration::from_secs(30))
                .resolve_to_addrs(&host, &first)
                .build()
                .map_err(|_| {
                    NumeraError::new("input_file_error", "Image transport cannot be initialized.")
                        .stage("download")
                })?;
            let mut response = client.get(url.clone()).send().await.map_err(|_| {
                NumeraError::new("input_file_error", "Image download interrupted.")
                    .stage("download")
            })?;
            let status = response.status().as_u16();
            if (300..400).contains(&status) {
                let location = response
                    .headers()
                    .get("location")
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| {
                        NumeraError::new("unsafe_destination", "Redirect has no destination.")
                    })?;
                url = url
                    .join(location)
                    .map_err(|_| NumeraError::new("unsafe_destination", "Unsafe image URL."))?;
                continue;
            }
            if !(200..300).contains(&status) {
                return Err(NumeraError::new(
                    "input_file_error",
                    &format!("Image download returned HTTP {status}."),
                )
                .stage("download"));
            }
            let mut bytes = BoundedBytes::new(
                response.content_length(),
                !matches!(status, 204 | 205),
                limit,
            )?;
            while let Some(chunk) = response.chunk().await.map_err(|_| {
                NumeraError::new("input_file_error", "Image download interrupted.")
                    .stage("download")
            })? {
                bytes.push(&chunk, None)?;
            }
            return Ok(bytes.finish());
        }
        Err(NumeraError::new(
            "unsafe_destination",
            "Too many image redirects.",
        ))
    };
    tokio::select! {
        _=cancel.cancelled()=>Err(NumeraError::new("cancelled","Waiting stopped; upstream completion and billing may be unknown.").stage("network")),
        result=tokio::time::timeout(Duration::from_secs(30),operation)=>result.map_err(|_|NumeraError::new("request_timeout","Image download deadline exceeded.").stage("download"))?,
    }
}
