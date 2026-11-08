//! BT-AUDIO-20261107: untrusted per-Hello cache claim, never NIC authority.
use super::{ordinary_ipv4, valid_mac, ServerLinkHint};
use serde::{
    de::{IgnoredAny, MapAccess, Visitor},
    ser::SerializeMap,
    Deserialize, Deserializer, Serialize, Serializer,
};
use std::{fmt, net::Ipv4Addr};

pub const CACHE_CAPABILITY: &str = "server-link-cache-v1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RememberedServerLink {
    #[default]
    Absent,
    Invalid,
    Value {
        server_ipv4: Ipv4Addr,
        mac: [u8; 6],
    },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WireClaim {
    version: u8,
    server_ipv4: Ipv4Addr,
    mac: String,
}

fn mac_bytes(text: &str) -> Option<[u8; 6]> {
    if text.len() != 17 {
        return None;
    }
    let mut result = [0; 6];
    for (n, part) in text.split(':').enumerate() {
        if n >= 6 || part.len() != 2 || !part.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        result[n] = u8::from_str_radix(part, 16).ok()?;
    }
    valid_mac(&result).then_some(result)
}

// Untagged buffering retains map entries (including duplicates), unlike a
// serde_json::Value/Object map. Strict WireClaim rejects duplicate/unknown keys;
// fallback consumes invalid optional data without rejecting the surrounding Hello.
#[derive(Deserialize)]
#[serde(untagged)]
enum OptionalClaim {
    Null(()),
    Value(WireClaim),
    Invalid(IgnoredAny),
}
impl OptionalClaim {
    fn validated(self) -> RememberedServerLink {
        match self {
            Self::Null(()) => RememberedServerLink::Absent,
            Self::Value(claim) if claim.version == 1 && ordinary_ipv4(claim.server_ipv4) => {
                match mac_bytes(&claim.mac) {
                    Some(mac) => RememberedServerLink::Value {
                        server_ipv4: claim.server_ipv4,
                        mac,
                    },
                    None => RememberedServerLink::Invalid,
                }
            }
            _ => RememberedServerLink::Invalid,
        }
    }
}

// This flattened adapter owns only its optional key; all other unknown Hello
// extensions retain their existing ignored behavior. Repeated optional keys are
// explicitly invalid even if identical. Known required Hello keys stay strict.
impl<'de> Deserialize<'de> for RememberedServerLink {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ClaimVisitor;
        impl<'de> Visitor<'de> for ClaimVisitor {
            type Value = RememberedServerLink;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("Hello extension fields")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut seen = false;
                let mut result = RememberedServerLink::Absent;
                while let Some(key) = map.next_key::<String>()? {
                    if key == "remembered_server_link" {
                        let value = map.next_value::<OptionalClaim>()?.validated();
                        result = if seen {
                            RememberedServerLink::Invalid
                        } else {
                            value
                        };
                        seen = true;
                    } else {
                        let _ = map.next_value::<IgnoredAny>()?;
                    }
                }
                Ok(result)
            }
        }
        deserializer.deserialize_map(ClaimVisitor)
    }
}
impl Serialize for RememberedServerLink {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map =
            serializer.serialize_map(Some(if matches!(self, Self::Absent) { 0 } else { 1 }))?;
        match self {
            Self::Absent => {}
            Self::Invalid => map.serialize_entry("remembered_server_link", &false)?,
            Self::Value { server_ipv4, mac } => {
                let value = WireClaim {
                    version: 1,
                    server_ipv4: *server_ipv4,
                    mac: mac
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<Vec<_>>()
                        .join(":"),
                };
                map.serialize_entry("remembered_server_link", &value)?;
            }
        }
        map.end()
    }
}

impl RememberedServerLink {
    pub(crate) fn state(self) -> &'static str {
        match self {
            Self::Absent => "absent",
            Self::Invalid => "invalid",
            Self::Value { .. } => "value",
        }
    }
    /// A comparison fact, never evidence that Welcome was sent or flash saved.
    /// The independent current hint is the only authority for the reply.
    pub(crate) fn compare(self, current: Option<&ServerLinkHint>) -> &'static str {
        let Some(current) = current.filter(|h| h.version == 1 && ordinary_ipv4(h.server_ipv4))
        else {
            return "unavailable";
        };
        let Some(actual_mac) = mac_bytes(&current.mac) else {
            return "unavailable";
        };
        match self {
            Self::Absent => "new",
            Self::Invalid => "invalid",
            Self::Value { server_ipv4, mac }
                if server_ipv4 != current.server_ipv4 || !valid_mac(&mac) =>
            {
                "invalid"
            }
            Self::Value { mac, .. } if mac == actual_mac => "matched",
            Self::Value { .. } => "mismatch",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{encode, Decoder, WireMessage, MAX_CONTROL_BYTES, VERSION};
    const VALID: &str = r#"{"version":1,"server_ipv4":"192.168.0.120","mac":"2C:CF:67:11:72:F3"}"#;
    fn json(extra: &str) -> String {
        format!(
            r#"{{"type":"hello","data":{{"device_id":"test","capabilities":["lcd","server-link-hint-v1","server-link-cache-v1"],"screen_owner_epoch":1{extra}}}}}"#
        )
    }
    fn claim(json: &str) -> RememberedServerLink {
        let message: WireMessage = serde_json::from_str(json).unwrap();
        match message {
            WireMessage::Hello {
                remembered_server_link,
                ..
            } => remembered_server_link,
            _ => panic!("Hello"),
        }
    }
    fn framed(payload: &str) -> Vec<u8> {
        let mut result = ((payload.len() + 2) as u32).to_be_bytes().to_vec();
        result.extend_from_slice(&[VERSION, 1]);
        result.extend_from_slice(payload.as_bytes());
        result
    }
    fn hint() -> ServerLinkHint {
        ServerLinkHint {
            version: 1,
            server_ipv4: "192.168.0.120".parse().unwrap(),
            client_ipv4: "192.168.0.44".parse().unwrap(),
            prefix_length: 24,
            mac: "2c:cf:67:11:72:f3".into(),
        }
    }
    #[test]
    fn server_link_remembered_legacy_unknown_fields_and_valid_roundtrip() {
        let old = r#"{"type":"hello","data":{"device_id":"old","capabilities":["lcd"],"screen_owner_epoch":1}}"#;
        let message: WireMessage = serde_json::from_str(old).unwrap();
        assert_eq!(serde_json::to_string(&message).unwrap(), old);
        assert_eq!(
            claim(&json(r#", "future":{"nested":[1,2,3]}"#)),
            RememberedServerLink::Absent
        );
        for extra in [
            "".into(),
            r#", "remembered_server_link":null"#.into(),
            format!(r#", "remembered_server_link":{VALID}"#),
        ] {
            let message: WireMessage = serde_json::from_str(&json(&extra)).unwrap();
            assert_eq!(
                Decoder::default().push(&encode(&message).unwrap()).unwrap(),
                vec![message]
            );
        }
        assert_eq!(
            claim(&json(&format!(r#", "remembered_server_link":{VALID}"#))).compare(Some(&hint())),
            "matched"
        );
    }
    #[test]
    fn server_link_remembered_invalid_optional_does_not_break_real_hello_decoder() {
        let cases = vec![
            "false".into(),
            "42".into(),
            r#""bad""#.into(),
            "[]".into(),
            "{}".into(),
            VALID.replace("\"version\":1", "\"version\":2"),
            VALID.replace("\"version\":1", "\"version\":1,\"version\":1"),
            VALID.replace("192.168.0.120", "127.0.0.1"),
            VALID.replace("192.168.0.120", "::1"),
            VALID.replace("2C:CF:67:11:72:F3", "00:00:00:00:00:00"),
            VALID.replace("2C:CF:67:11:72:F3", "ff:ff:ff:ff:ff:ff"),
            VALID.replace("2C:CF:67:11:72:F3", "2C:CF:67:11:72:F"),
            VALID.replace("2C:CF:67:11:72:F3", "2C:CF:67:11:72:zz"),
            VALID.replace("\"version\":1", "\"version\":{\"version\":1}"),
            VALID.replace("\"version\":1", "\"unknown\":0,\"version\":1"),
        ];
        for invalid in cases {
            let raw = json(&format!(r#", "remembered_server_link":{invalid}"#));
            assert_eq!(claim(&raw), RememberedServerLink::Invalid, "{raw}");
            assert!(matches!(
                &Decoder::default().push(&framed(&raw)).unwrap()[0],
                WireMessage::Hello {
                    remembered_server_link: RememberedServerLink::Invalid,
                    ..
                }
            ));
        }
        for extra in [
            format!(r#", "remembered_server_link":{VALID},"remembered_server_link":{VALID}"#),
            format!(r#", "remembered_server_link":null,"remembered_server_link":{VALID}"#),
            format!(
                r#", "remembered_server_link":{VALID},"remembered_server_link":null,"remembered_server_link":{VALID}"#
            ),
        ] {
            let raw = json(&extra);
            assert_eq!(
                claim(&raw),
                RememberedServerLink::Invalid,
                "duplicate claim lost"
            );
            assert!(matches!(
                &Decoder::default().push(&framed(&raw)).unwrap()[0],
                WireMessage::Hello {
                    remembered_server_link: RememberedServerLink::Invalid,
                    ..
                }
            ));
        }
        assert!(
            serde_json::from_str::<WireMessage>(&json(r#", "device_id":"duplicate""#)).is_err()
        );
        let oversized = json(&format!(
            r#", "remembered_server_link":{{"mac":"{}"}}"#,
            "x".repeat(MAX_CONTROL_BYTES)
        ));
        assert!(
            Decoder::default().push(&framed(&oversized)).is_err(),
            "do not widen existing message size limit"
        );
    }
    #[test]
    fn server_link_remembered_compare_never_adopts_claim_or_crosses_endpoint() {
        let value = claim(&json(&format!(r#", "remembered_server_link":{VALID}"#)));
        let current = hint();
        assert_eq!(value.compare(Some(&current)), "matched");
        let mut changed = current.clone();
        changed.mac = "02:03:04:05:06:07".into();
        assert_eq!(value.compare(Some(&changed)), "mismatch");
        changed.server_ipv4 = "192.168.0.121".parse().unwrap();
        assert_eq!(value.compare(Some(&changed)), "invalid");
        assert_eq!(value.compare(None), "unavailable");
        assert_eq!(
            RememberedServerLink::Invalid.compare(Some(&current)),
            "invalid"
        );
        assert_eq!(RememberedServerLink::Absent.compare(Some(&current)), "new");
        assert_eq!(
            current,
            hint(),
            "comparison never mutates independently obtained current value"
        );
    }
}
