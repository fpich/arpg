//! Compact deterministic codec for trade intents in replay entries
//! (SPEC.md sections 116-118, 159). Encoding is canonical so replays
//! stay byte-identical for identical command streams.

use arpg_sim::command::TradeIntent;

/// Encode a trade intent into (op, trade_id, payload) where payload is a
/// canonical little-endian byte string.
pub fn encode(intent: &TradeIntent) -> (u8, u64, Vec<u8>) {
    match intent {
        TradeIntent::Open { target } => (0, 0, target.0.to_le_bytes().to_vec()),
        TradeIntent::SetOffer { trade, items, gold } => {
            let mut buf = Vec::with_capacity(8 + 4 + items.len() * 16);
            buf.extend_from_slice(&(items.len() as u32).to_le_bytes());
            for item in items {
                buf.extend_from_slice(&item.0.to_le_bytes());
            }
            buf.extend_from_slice(&gold.to_le_bytes());
            (1, *trade, buf)
        }
        TradeIntent::Accept { trade } => (2, *trade, Vec::new()),
        TradeIntent::Cancel { trade } => (3, *trade, Vec::new()),
    }
}

/// Decode a trade intent from its canonical encoding.
pub fn decode(op: u8, trade: u64, payload: &[u8]) -> Result<TradeIntent, &'static str> {
    match op {
        0 => {
            if payload.len() != 4 {
                return Err("trade open payload must be 4 bytes");
            }
            Ok(TradeIntent::Open {
                target: arpg_core::PlayerId(u32::from_le_bytes(payload[..4].try_into().unwrap())),
            })
        }
        1 => {
            if payload.len() < 4 {
                return Err("trade offer payload too short");
            }
            let count = u32::from_le_bytes(payload[..4].try_into().unwrap()) as usize;
            let expected = 4 + count * 16 + 8;
            if payload.len() != expected {
                return Err("trade offer payload length mismatch");
            }
            let mut items = Vec::with_capacity(count);
            for i in 0..count {
                let off = 4 + i * 16;
                items.push(arpg_core::ItemId(u128::from_le_bytes(
                    payload[off..off + 16].try_into().unwrap(),
                )));
            }
            let gold = u64::from_le_bytes(payload[4 + count * 16..].try_into().unwrap());
            Ok(TradeIntent::SetOffer { trade, items, gold })
        }
        2 if payload.is_empty() => Ok(TradeIntent::Accept { trade }),
        3 if payload.is_empty() => Ok(TradeIntent::Cancel { trade }),
        _ => Err("unknown trade op"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let intents = vec![
            TradeIntent::Open {
                target: arpg_core::PlayerId(7),
            },
            TradeIntent::SetOffer {
                trade: 3,
                items: vec![arpg_core::ItemId(42), arpg_core::ItemId(9)],
                gold: 500,
            },
            TradeIntent::Accept { trade: 3 },
            TradeIntent::Cancel { trade: 3 },
        ];
        for i in &intents {
            let (op, trade, payload) = encode(i);
            let back = decode(op, trade, &payload).unwrap();
            assert_eq!(&back, i);
        }
    }

    #[test]
    fn reject_truncated() {
        assert!(decode(1, 1, &[0, 0]).is_err());
        assert!(decode(9, 0, &[]).is_err());
    }
}
